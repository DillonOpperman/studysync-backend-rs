//! NomicBert (nomic-embed-text-v1.5). RoPE, fused QKV, SwiGLU. Adapted from the
//! Candle nomic-bert example (Apache-2.0).

use candle_core::{DType, Device, Result, Tensor, D};
use candle_nn::{
    embedding, layer_norm, linear, linear_no_bias, Embedding, LayerNorm, Linear, Module, VarBuilder,
};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub vocab_size: usize,
    pub n_embd: usize,
    pub n_head: usize,
    pub n_layer: usize,
    pub n_inner: usize,
    pub n_positions: usize,
    pub type_vocab_size: usize,
    pub layer_norm_epsilon: f64,
    pub rotary_emb_fraction: f64,
    pub rotary_emb_base: f64,
    pub rotary_emb_interleaved: bool,
    pub qkv_proj_bias: bool,
    pub mlp_fc1_bias: bool,
    pub mlp_fc2_bias: bool,
    pub activation_function: String,
    pub prenorm: bool,
    pub model_type: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            vocab_size: 30528,
            n_embd: 768,
            n_head: 12,
            n_layer: 12,
            n_inner: 3072,
            n_positions: 8192,
            type_vocab_size: 2,
            layer_norm_epsilon: 1e-12,
            rotary_emb_fraction: 1.0,
            rotary_emb_base: 1000.0,
            rotary_emb_interleaved: false,
            qkv_proj_bias: false,
            mlp_fc1_bias: false,
            mlp_fc2_bias: false,
            activation_function: "swiglu".to_string(),
            prenorm: false,
            model_type: Some("nomic_bert".to_string()),
        }
    }
}

impl Config {
    fn head_dim(&self) -> usize {
        self.n_embd / self.n_head
    }

    fn rotary_emb_dim(&self) -> usize {
        (self.head_dim() as f64 * self.rotary_emb_fraction) as usize
    }
}

struct RotaryEmbedding {
    cos: Tensor,
    sin: Tensor,
}

impl RotaryEmbedding {
    fn new(dim: usize, max_seq_len: usize, base: f64, device: &Device) -> Result<Self> {
        let half_dim = dim / 2;
        let inv_freq: Vec<f32> = (0..half_dim)
            .map(|i| 1f32 / (base as f32).powf(2.0 * i as f32 / dim as f32))
            .collect();
        let inv_freq = Tensor::new(inv_freq.as_slice(), device)?;
        let positions = Tensor::arange(0u32, max_seq_len as u32, device)?
            .to_dtype(DType::F32)?
            .reshape((max_seq_len, 1))?;
        let freqs = positions.matmul(&inv_freq.unsqueeze(0)?)?;
        Ok(Self {
            cos: freqs.cos()?,
            sin: freqs.sin()?,
        })
    }

    fn apply(&self, x: &Tensor) -> Result<Tensor> {
        let (_, _, seq_len, head_dim) = x.dims4()?;
        let half = head_dim / 2;
        let cos = self
            .cos
            .narrow(0, 0, seq_len)?
            .to_dtype(x.dtype())?
            .reshape((1, 1, seq_len, half))?;
        let sin = self
            .sin
            .narrow(0, 0, seq_len)?
            .to_dtype(x.dtype())?
            .reshape((1, 1, seq_len, half))?;
        let x1 = x.narrow(D::Minus1, 0, half)?;
        let x2 = x.narrow(D::Minus1, half, half)?;
        let out1 = (x1.broadcast_mul(&cos)? - x2.broadcast_mul(&sin)?)?;
        let out2 = (x1.broadcast_mul(&sin)? + x2.broadcast_mul(&cos)?)?;
        Tensor::cat(&[&out1, &out2], D::Minus1)
    }
}

struct NomicBertEmbeddings {
    word_embeddings: Embedding,
    token_type_embeddings: Option<Embedding>,
}

impl NomicBertEmbeddings {
    fn new(vb: VarBuilder, config: &Config) -> Result<Self> {
        let word_embeddings =
            embedding(config.vocab_size, config.n_embd, vb.pp("word_embeddings"))?;
        let token_type_embeddings = if config.type_vocab_size > 0 {
            Some(embedding(
                config.type_vocab_size,
                config.n_embd,
                vb.pp("token_type_embeddings"),
            )?)
        } else {
            None
        };
        Ok(Self {
            word_embeddings,
            token_type_embeddings,
        })
    }

    fn forward(&self, input_ids: &Tensor, token_type_ids: Option<&Tensor>) -> Result<Tensor> {
        let embeddings = self.word_embeddings.forward(input_ids)?;
        if let Some(tte) = &self.token_type_embeddings {
            let tt_ids = match token_type_ids {
                Some(ids) => ids.clone(),
                None => {
                    let (b, s) = input_ids.dims2()?;
                    Tensor::zeros((b, s), DType::U32, input_ids.device())?
                }
            };
            embeddings + tte.forward(&tt_ids)?
        } else {
            Ok(embeddings)
        }
    }
}

struct NomicBertAttention {
    wqkv: Linear,
    out_proj: Linear,
    num_heads: usize,
    head_dim: usize,
    n_embd: usize,
}

impl NomicBertAttention {
    fn new(vb: VarBuilder, config: &Config) -> Result<Self> {
        let wqkv = if config.qkv_proj_bias {
            linear(config.n_embd, 3 * config.n_embd, vb.pp("Wqkv"))?
        } else {
            linear_no_bias(config.n_embd, 3 * config.n_embd, vb.pp("Wqkv"))?
        };
        let out_proj = if config.qkv_proj_bias {
            linear(config.n_embd, config.n_embd, vb.pp("out_proj"))?
        } else {
            linear_no_bias(config.n_embd, config.n_embd, vb.pp("out_proj"))?
        };
        Ok(Self {
            wqkv,
            out_proj,
            num_heads: config.n_head,
            head_dim: config.head_dim(),
            n_embd: config.n_embd,
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: &Tensor,
        rotary_emb: &RotaryEmbedding,
    ) -> Result<Tensor> {
        let (batch_size, seq_len, _) = hidden_states.dims3()?;
        let qkv = self.wqkv.forward(hidden_states)?;
        let q = qkv.narrow(D::Minus1, 0, self.n_embd)?;
        let k = qkv.narrow(D::Minus1, self.n_embd, self.n_embd)?;
        let v = qkv.narrow(D::Minus1, 2 * self.n_embd, self.n_embd)?;
        let q = q
            .reshape((batch_size, seq_len, self.num_heads, self.head_dim))?
            .transpose(1, 2)?
            .contiguous()?;
        let k = k
            .reshape((batch_size, seq_len, self.num_heads, self.head_dim))?
            .transpose(1, 2)?
            .contiguous()?;
        let v = v
            .reshape((batch_size, seq_len, self.num_heads, self.head_dim))?
            .transpose(1, 2)?;
        let q = rotary_emb.apply(&q)?;
        let k = rotary_emb.apply(&k)?;
        let scale = (self.head_dim as f64).sqrt();
        let attn_scores = (q.matmul(&k.t()?)? / scale)?;
        let attn_scores = attn_scores.broadcast_add(attention_mask)?;
        let attn_probs = candle_nn::ops::softmax_last_dim(&attn_scores)?;
        let attn_output = attn_probs.matmul(&v.contiguous()?)?;
        let attn_output = attn_output
            .transpose(1, 2)?
            .contiguous()?
            .flatten_from(D::Minus2)?;
        self.out_proj.forward(&attn_output)
    }
}

struct NomicBertSwiGLU {
    fc11: Linear,
    fc12: Linear,
    fc2: Linear,
}

impl NomicBertSwiGLU {
    fn new(vb: VarBuilder, config: &Config) -> Result<Self> {
        let (fc11, fc12) = if config.mlp_fc1_bias {
            (
                linear(config.n_embd, config.n_inner, vb.pp("fc11"))?,
                linear(config.n_embd, config.n_inner, vb.pp("fc12"))?,
            )
        } else {
            (
                linear_no_bias(config.n_embd, config.n_inner, vb.pp("fc11"))?,
                linear_no_bias(config.n_embd, config.n_inner, vb.pp("fc12"))?,
            )
        };
        let fc2 = if config.mlp_fc2_bias {
            linear(config.n_inner, config.n_embd, vb.pp("fc2"))?
        } else {
            linear_no_bias(config.n_inner, config.n_embd, vb.pp("fc2"))?
        };
        Ok(Self { fc11, fc12, fc2 })
    }
}

impl Module for NomicBertSwiGLU {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let y = self.fc11.forward(xs)?;
        let gate = self.fc12.forward(xs)?.silu()?;
        self.fc2.forward(&(y * gate)?)
    }
}

struct NomicBertBlock {
    attn: NomicBertAttention,
    mlp: NomicBertSwiGLU,
    norm1: LayerNorm,
    norm2: LayerNorm,
    prenorm: bool,
}

impl NomicBertBlock {
    fn new(vb: VarBuilder, config: &Config) -> Result<Self> {
        Ok(Self {
            attn: NomicBertAttention::new(vb.pp("attn"), config)?,
            mlp: NomicBertSwiGLU::new(vb.pp("mlp"), config)?,
            norm1: layer_norm(config.n_embd, config.layer_norm_epsilon, vb.pp("norm1"))?,
            norm2: layer_norm(config.n_embd, config.layer_norm_epsilon, vb.pp("norm2"))?,
            prenorm: config.prenorm,
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: &Tensor,
        rotary_emb: &RotaryEmbedding,
    ) -> Result<Tensor> {
        if self.prenorm {
            let residual = hidden_states;
            let hidden_states = self.norm1.forward(hidden_states)?;
            let attn_out = self
                .attn
                .forward(&hidden_states, attention_mask, rotary_emb)?;
            let hidden_states = (residual + attn_out)?;
            let residual = hidden_states.clone();
            let hidden_states = self.norm2.forward(&hidden_states)?;
            let mlp_out = self.mlp.forward(&hidden_states)?;
            residual + mlp_out
        } else {
            let attn_out = self
                .attn
                .forward(hidden_states, attention_mask, rotary_emb)?;
            let hidden_states = self.norm1.forward(&(hidden_states + attn_out)?)?;
            let mlp_out = self.mlp.forward(&hidden_states)?;
            self.norm2.forward(&(hidden_states + mlp_out)?)
        }
    }
}

struct NomicBertEncoder {
    layers: Vec<NomicBertBlock>,
    rotary_emb: RotaryEmbedding,
}

impl NomicBertEncoder {
    fn new(vb: VarBuilder, config: &Config) -> Result<Self> {
        let layers = (0..config.n_layer)
            .map(|i| NomicBertBlock::new(vb.pp(format!("layers.{i}")), config))
            .collect::<Result<Vec<_>>>()?;
        let rotary_emb = RotaryEmbedding::new(
            config.rotary_emb_dim(),
            config.n_positions.min(2048),
            config.rotary_emb_base,
            vb.device(),
        )?;
        Ok(Self { layers, rotary_emb })
    }

    fn forward(&self, hidden_states: &Tensor, attention_mask: &Tensor) -> Result<Tensor> {
        let mut xs = hidden_states.clone();
        for layer in &self.layers {
            xs = layer.forward(&xs, attention_mask, &self.rotary_emb)?;
        }
        Ok(xs)
    }
}

fn get_extended_attention_mask(attention_mask: &Tensor, dtype: DType) -> Result<Tensor> {
    let mask = attention_mask
        .to_dtype(DType::F32)?
        .unsqueeze(1)?
        .unsqueeze(1)?;
    let ones = Tensor::ones_like(&mask)?;
    let pad = ones.sub(&mask)?;
    let additive = pad.broadcast_mul(&Tensor::new(-1e4f32, mask.device())?)?;
    additive.to_dtype(dtype)
}

pub struct NomicBertModel {
    embeddings: NomicBertEmbeddings,
    emb_ln: LayerNorm,
    encoder: NomicBertEncoder,
}

impl NomicBertModel {
    pub fn load(vb: VarBuilder, config: &Config) -> Result<Self> {
        let load_inner = |vb: VarBuilder| -> Result<Self> {
            Ok(Self {
                embeddings: NomicBertEmbeddings::new(vb.pp("embeddings"), config)?,
                emb_ln: layer_norm(config.n_embd, config.layer_norm_epsilon, vb.pp("emb_ln"))?,
                encoder: NomicBertEncoder::new(vb.pp("encoder"), config)?,
            })
        };
        load_inner(vb.clone()).or_else(|err| {
            if let Some(model_type) = &config.model_type {
                load_inner(vb.pp(model_type)).map_err(|_| err)
            } else {
                Err(err)
            }
        })
    }

    pub fn forward(
        &self,
        input_ids: &Tensor,
        token_type_ids: Option<&Tensor>,
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let hidden_states = self.embeddings.forward(input_ids, token_type_ids)?;
        let hidden_states = self.emb_ln.forward(&hidden_states)?;
        let attention_mask = match attention_mask {
            Some(mask) => mask.clone(),
            None => input_ids.ones_like()?,
        };
        let extended_mask = get_extended_attention_mask(&attention_mask, hidden_states.dtype())?;
        self.encoder.forward(&hidden_states, &extended_mask)
    }
}
