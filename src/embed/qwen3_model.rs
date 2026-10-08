//! Qwen3 text embedding transformer (Candle). Adapted from fastembed's Apache-2.0
//! Qwen3 implementation, text-only, without ONNX Runtime.

use candle_core::{DType, Device, IndexOp, Result, Tensor, D};
use candle_nn::{linear, linear_no_bias, Activation, Linear, Module, VarBuilder};
use serde::Deserialize;

fn scalar_f32(device: &Device, value: f32) -> Result<Tensor> {
    Tensor::from_slice(&[value], (1,), device)?.to_dtype(DType::F32)
}

fn scalar_f64_as_f32(device: &Device, value: f64) -> Result<Tensor> {
    scalar_f32(device, value as f32)
}

fn default_true() -> bool {
    true
}

#[allow(dead_code)]
#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub attention_bias: bool,
    pub attention_dropout: f32,
    pub head_dim: Option<usize>,
    pub hidden_act: Activation,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub max_position_embeddings: usize,
    pub num_attention_heads: usize,
    pub num_hidden_layers: usize,
    pub num_key_value_heads: usize,
    pub rms_norm_eps: f64,
    pub rope_scaling: Option<serde_json::Value>,
    pub rope_theta: f64,
    pub sliding_window: Option<usize>,
    pub tie_word_embeddings: bool,
    #[serde(default = "default_true")]
    pub use_cache: bool,
    #[serde(default)]
    pub use_sliding_window: bool,
    pub vocab_size: usize,
    #[serde(default)]
    pub max_window_layers: usize,
}

impl Config {
    pub fn head_dim(&self) -> usize {
        self.head_dim
            .unwrap_or(self.hidden_size / self.num_attention_heads)
    }

    pub fn num_kv_groups(&self) -> usize {
        self.num_attention_heads / self.num_key_value_heads
    }
}

fn build_attention_mask_4d(attention_mask_2d: &Tensor) -> Result<Tensor> {
    let (batch_size, seq_len) = attention_mask_2d.dims2()?;
    let device = attention_mask_2d.device();
    let mask_value = -1e4f32;
    let mut data = vec![0.0f32; seq_len * seq_len];
    for i in 0..seq_len {
        for j in (i + 1)..seq_len {
            data[i * seq_len + j] = mask_value;
        }
    }
    let causal = Tensor::from_vec(data, (1, 1, seq_len, seq_len), device)?;
    let pad_mask_expanded = attention_mask_2d.unsqueeze(1)?.unsqueeze(2)?;
    let pad_mask_expanded = pad_mask_expanded.expand((batch_size, 1, seq_len, seq_len))?;
    let pad_mask_f32 = pad_mask_expanded.to_dtype(DType::F32)?;
    let ones = Tensor::ones_like(&pad_mask_f32)?;
    let inverted_mask = ones.sub(&pad_mask_f32)?;
    let mask_val_t = Tensor::new(&[mask_value], device)?;
    let pad_additive = inverted_mask.broadcast_mul(&mask_val_t)?;
    let causal_broadcast = causal.broadcast_as((batch_size, 1, seq_len, seq_len))?;
    causal_broadcast.add(&pad_additive)
}

fn last_token_pool(hidden: &Tensor, attention_mask_2d: &Tensor) -> Result<Tensor> {
    let (batch_size, seq_len, _) = hidden.dims3()?;
    let masks = attention_mask_2d.to_vec2::<f32>()?;
    if masks.len() != batch_size {
        return Err(candle_core::Error::Msg(
            "attention mask batch size mismatch".into(),
        ));
    }
    let mut pooled_rows = Vec::with_capacity(batch_size);
    for (batch_idx, row) in masks.iter().enumerate() {
        let last_idx = row.iter().rposition(|&value| value > 0.0).unwrap_or(seq_len - 1);
        pooled_rows.push(hidden.i((batch_idx, last_idx))?);
    }
    let pooled_refs: Vec<&Tensor> = pooled_rows.iter().collect();
    Tensor::stack(&pooled_refs, 0)
}

fn l2_normalize(xs: &Tensor) -> Result<Tensor> {
    let sum_sq = xs.sqr()?.sum_keepdim(1)?;
    let eps_tensor = Tensor::new(&[1e-12f32], xs.device())?
        .to_dtype(sum_sq.dtype())?
        .broadcast_as(sum_sq.shape())?;
    let norm = sum_sq.add(&eps_tensor)?.sqrt()?;
    xs.broadcast_div(&norm)
}

struct Qwen3RMSNorm {
    weight: Tensor,
    eps: f64,
}

impl Qwen3RMSNorm {
    fn new(dim: usize, eps: f64, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            weight: vb.get((dim,), "weight")?,
            eps,
        })
    }
}

impl Module for Qwen3RMSNorm {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let in_dtype = xs.dtype();
        let xs_f = xs.to_dtype(DType::F32)?;
        let var = xs_f.powf(2.0)?.mean_keepdim(D::Minus1)?;
        let eps_t = scalar_f64_as_f32(xs.device(), self.eps)?;
        let inv_rms = var.broadcast_add(&eps_t)?.sqrt()?.recip()?;
        let normed = xs_f.broadcast_mul(&inv_rms)?.to_dtype(in_dtype)?;
        let weight = self.weight.to_dtype(in_dtype)?;
        normed.broadcast_mul(&weight)
    }
}

struct Qwen3MLP {
    gate_proj: Linear,
    up_proj: Linear,
    down_proj: Linear,
    act_fn: Activation,
}

impl Qwen3MLP {
    fn new(cfg: &Config, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            gate_proj: linear_no_bias(cfg.hidden_size, cfg.intermediate_size, vb.pp("gate_proj"))?,
            up_proj: linear_no_bias(cfg.hidden_size, cfg.intermediate_size, vb.pp("up_proj"))?,
            down_proj: linear_no_bias(cfg.intermediate_size, cfg.hidden_size, vb.pp("down_proj"))?,
            act_fn: cfg.hidden_act,
        })
    }
}

impl Module for Qwen3MLP {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let lhs = xs.apply(&self.gate_proj)?.apply(&self.act_fn)?;
        let rhs = xs.apply(&self.up_proj)?;
        (lhs * rhs)?.apply(&self.down_proj)
    }
}

struct Qwen3RotaryEmbedding {
    inv_freq: Tensor,
    attention_factor: f32,
}

impl Qwen3RotaryEmbedding {
    fn new(cfg: &Config, device: &Device) -> Result<Self> {
        let dim = cfg.head_dim();
        let t = Tensor::arange_step(0u32, dim as u32, 2u32, device)?.to_dtype(DType::F32)?;
        let dim_t = scalar_f32(device, dim as f32)?;
        let exponent = t.broadcast_div(&dim_t)?;
        let ln_base_t = scalar_f32(device, (cfg.rope_theta as f32).ln())?;
        let inv_freq = exponent.broadcast_mul(&ln_base_t.neg()?)?.exp()?;
        Ok(Self {
            inv_freq,
            attention_factor: 1.0,
        })
    }

    fn forward(&self, xs: &Tensor, position_ids: &Tensor) -> Result<(Tensor, Tensor)> {
        let (batch, seq) = position_ids.dims2()?;
        let inv_freq = self.inv_freq.to_device(xs.device())?.to_dtype(DType::F32)?;
        let half = inv_freq.dims1()?;
        let inv_freq_expanded = inv_freq
            .reshape((1, half, 1))?
            .expand((batch, half, 1))?
            .contiguous()?;
        let pos = position_ids
            .to_dtype(DType::F32)?
            .reshape((batch, 1, seq))?
            .contiguous()?;
        let freqs = inv_freq_expanded.matmul(&pos)?.transpose(1, 2)?.contiguous()?;
        let emb = Tensor::cat(&[&freqs, &freqs], 2)?;
        let scale = scalar_f32(xs.device(), self.attention_factor)?;
        let cos = emb.cos()?.broadcast_mul(&scale)?.to_dtype(xs.dtype())?;
        let sin = emb.sin()?.broadcast_mul(&scale)?.to_dtype(xs.dtype())?;
        Ok((cos, sin))
    }
}

fn rotate_half(x: &Tensor) -> Result<Tensor> {
    let dim = *x.dims().last().unwrap();
    let half = dim / 2;
    let x1 = x.narrow(D::Minus1, 0, half)?;
    let x2 = x.narrow(D::Minus1, half, half)?;
    Tensor::cat(&[&x2.neg()?, &x1], x.rank() - 1)
}

fn apply_rotary_pos_emb(
    q: &Tensor,
    k: &Tensor,
    cos: &Tensor,
    sin: &Tensor,
) -> Result<(Tensor, Tensor)> {
    let cos_u = cos.unsqueeze(1)?;
    let sin_u = sin.unsqueeze(1)?;
    let q_embed = (q.broadcast_mul(&cos_u)? + rotate_half(q)?.broadcast_mul(&sin_u)?)?;
    let k_embed = (k.broadcast_mul(&cos_u)? + rotate_half(k)?.broadcast_mul(&sin_u)?)?;
    Ok((q_embed, k_embed))
}

fn repeat_kv(x: &Tensor, n_rep: usize) -> Result<Tensor> {
    if n_rep == 1 {
        return Ok(x.clone());
    }
    let (b, n_kv, t, d) = x.dims4()?;
    x.unsqueeze(2)?
        .broadcast_as((b, n_kv, n_rep, t, d))?
        .reshape((b, n_kv * n_rep, t, d))
}

struct Qwen3Attention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    o_proj: Linear,
    q_norm: Qwen3RMSNorm,
    k_norm: Qwen3RMSNorm,
    num_heads: usize,
    num_kv_heads: usize,
    num_kv_groups: usize,
    head_dim: usize,
    scaling: f32,
}

impl Qwen3Attention {
    fn new(cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let head_dim = cfg.head_dim();
        let q_out = cfg.num_attention_heads * head_dim;
        let kv_out = cfg.num_key_value_heads * head_dim;
        let proj = |out, name| {
            if cfg.attention_bias {
                linear(cfg.hidden_size, out, vb.pp(name))
            } else {
                linear_no_bias(cfg.hidden_size, out, vb.pp(name))
            }
        };
        let o_proj = if cfg.attention_bias {
            linear(q_out, cfg.hidden_size, vb.pp("o_proj"))?
        } else {
            linear_no_bias(q_out, cfg.hidden_size, vb.pp("o_proj"))?
        };
        Ok(Self {
            q_proj: proj(q_out, "q_proj")?,
            k_proj: proj(kv_out, "k_proj")?,
            v_proj: proj(kv_out, "v_proj")?,
            o_proj,
            q_norm: Qwen3RMSNorm::new(head_dim, cfg.rms_norm_eps, vb.pp("q_norm"))?,
            k_norm: Qwen3RMSNorm::new(head_dim, cfg.rms_norm_eps, vb.pp("k_norm"))?,
            num_heads: cfg.num_attention_heads,
            num_kv_heads: cfg.num_key_value_heads,
            num_kv_groups: cfg.num_kv_groups(),
            head_dim,
            scaling: (head_dim as f32).powf(-0.5),
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        position_embeddings: (&Tensor, &Tensor),
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let (b, t, _) = hidden_states.dims3()?;
        let d = self.head_dim;
        let q = hidden_states
            .apply(&self.q_proj)?
            .reshape((b, t, self.num_heads, d))?
            .apply(&self.q_norm)?
            .transpose(1, 2)?;
        let k = hidden_states
            .apply(&self.k_proj)?
            .reshape((b, t, self.num_kv_heads, d))?
            .apply(&self.k_norm)?
            .transpose(1, 2)?;
        let v = hidden_states
            .apply(&self.v_proj)?
            .reshape((b, t, self.num_kv_heads, d))?
            .transpose(1, 2)?;
        let (cos, sin) = position_embeddings;
        let (q, k) = apply_rotary_pos_emb(&q, &k, cos, sin)?;
        let k = repeat_kv(&k, self.num_kv_groups)?;
        let v = repeat_kv(&v, self.num_kv_groups)?;
        let mut attn = q.matmul(&k.transpose(2, 3)?)?;
        let scale = scalar_f32(attn.device(), self.scaling)?.to_dtype(attn.dtype())?;
        attn = attn.broadcast_mul(&scale)?;
        if let Some(mask) = attention_mask {
            attn = attn.broadcast_add(&mask.to_dtype(attn.dtype())?)?;
        }
        let out = candle_nn::ops::softmax(&attn, D::Minus1)?.matmul(&v)?;
        out.transpose(1, 2)?
            .reshape((b, t, self.num_heads * d))?
            .apply(&self.o_proj)
    }
}

struct Qwen3DecoderLayer {
    self_attn: Qwen3Attention,
    mlp: Qwen3MLP,
    input_layernorm: Qwen3RMSNorm,
    post_attention_layernorm: Qwen3RMSNorm,
}

impl Qwen3DecoderLayer {
    fn new(cfg: &Config, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            self_attn: Qwen3Attention::new(cfg, vb.pp("self_attn"))?,
            mlp: Qwen3MLP::new(cfg, vb.pp("mlp"))?,
            input_layernorm: Qwen3RMSNorm::new(
                cfg.hidden_size,
                cfg.rms_norm_eps,
                vb.pp("input_layernorm"),
            )?,
            post_attention_layernorm: Qwen3RMSNorm::new(
                cfg.hidden_size,
                cfg.rms_norm_eps,
                vb.pp("post_attention_layernorm"),
            )?,
        })
    }

    fn forward(
        &self,
        hidden_states: &Tensor,
        attention_mask: Option<&Tensor>,
        position_embeddings: (&Tensor, &Tensor),
    ) -> Result<Tensor> {
        let residual = hidden_states.clone();
        let hs = hidden_states.apply(&self.input_layernorm)?;
        let hs = self
            .self_attn
            .forward(&hs, position_embeddings, attention_mask)?;
        let hs = (residual + hs)?;
        let residual = hs.clone();
        residual + hs.apply(&self.post_attention_layernorm)?.apply(&self.mlp)?
    }
}

pub struct Qwen3Model {
    embed_tokens: candle_nn::Embedding,
    layers: Vec<Qwen3DecoderLayer>,
    norm: Qwen3RMSNorm,
    rotary_emb: Qwen3RotaryEmbedding,
    device: Device,
}

impl Qwen3Model {
    pub fn new(cfg: Config, vb: VarBuilder) -> Result<Self> {
        let device = vb.device().clone();
        let embed_tokens =
            candle_nn::embedding(cfg.vocab_size, cfg.hidden_size, vb.pp("embed_tokens"))?;
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        for i in 0..cfg.num_hidden_layers {
            layers.push(Qwen3DecoderLayer::new(&cfg, vb.pp(format!("layers.{i}")))?);
        }
        Ok(Self {
            embed_tokens,
            layers,
            norm: Qwen3RMSNorm::new(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("norm"))?,
            rotary_emb: Qwen3RotaryEmbedding::new(&cfg, &device)?,
            device,
        })
    }

    pub fn forward(&self, input_ids: &Tensor, attention_mask_4d: Option<&Tensor>) -> Result<Tensor> {
        let hs = self.embed_tokens.forward(input_ids)?;
        let (b, t, _) = hs.dims3()?;
        let pos = Tensor::arange(0u32, t as u32, hs.device())?
            .unsqueeze(0)?
            .expand((b, t))?
            .contiguous()?;
        let (cos, sin) = self.rotary_emb.forward(&hs, &pos)?;
        let mut hs = hs;
        for layer in &self.layers {
            hs = layer.forward(&hs, attention_mask_4d, (&cos, &sin))?;
        }
        hs.apply(&self.norm)
    }

    pub fn embed_normalized(
        &self,
        input_ids: &Tensor,
        attention_mask_2d: &Tensor,
    ) -> Result<Vec<Vec<f32>>> {
        let mask4 = build_attention_mask_4d(attention_mask_2d)?;
        let hidden = self.forward(input_ids, Some(&mask4))?;
        let pooled = last_token_pool(&hidden, attention_mask_2d)?;
        l2_normalize(&pooled)?.to_dtype(DType::F32)?.to_vec2::<f32>()
    }

    pub fn device(&self) -> &Device {
        &self.device
    }
}
