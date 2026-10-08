use candle_core::{DType, Tensor};

use super::EmbeddingError;

pub fn mean_pool_l2(hidden: &Tensor, attention: &Tensor) -> Result<Vec<Vec<f32>>, EmbeddingError> {
    let mask = attention
        .to_dtype(DType::F32)
        .map_err(unavail)?
        .unsqueeze(2)
        .map_err(unavail)?;
    let summed = hidden
        .broadcast_mul(&mask)
        .map_err(unavail)?
        .sum(1)
        .map_err(unavail)?;
    let counts = mask
        .sum(1)
        .map_err(unavail)?
        .clamp(1e-9, f32::MAX)
        .map_err(unavail)?;
    let pooled = summed.broadcast_div(&counts).map_err(unavail)?;
    l2_rows(&pooled)
}

/// Official nomic-embed-text-v1.5 Transformers path: mean-pool, then
/// unparameterized layer-norm, then L2. Matryoshka truncation is not applied
/// (full 768-d).
pub fn mean_pool_layer_norm_l2(
    hidden: &Tensor,
    attention: &Tensor,
) -> Result<Vec<Vec<f32>>, EmbeddingError> {
    let mask = attention
        .to_dtype(DType::F32)
        .map_err(unavail)?
        .unsqueeze(2)
        .map_err(unavail)?;
    let summed = hidden
        .broadcast_mul(&mask)
        .map_err(unavail)?
        .sum(1)
        .map_err(unavail)?;
    let counts = mask
        .sum(1)
        .map_err(unavail)?
        .clamp(1e-9, f32::MAX)
        .map_err(unavail)?;
    let pooled = summed.broadcast_div(&counts).map_err(unavail)?;
    let mean = pooled.mean_keepdim(1).map_err(unavail)?;
    let centered = pooled.broadcast_sub(&mean).map_err(unavail)?;
    let var = centered
        .sqr()
        .map_err(unavail)?
        .mean_keepdim(1)
        .map_err(unavail)?;
    let denom = (var + 1e-5)
        .map_err(unavail)?
        .sqrt()
        .map_err(unavail)?
        .clamp(1e-12, f32::MAX)
        .map_err(unavail)?;
    let normalized = centered.broadcast_div(&denom).map_err(unavail)?;
    l2_rows(&normalized)
}

fn l2_rows(pooled: &Tensor) -> Result<Vec<Vec<f32>>, EmbeddingError> {
    let norm = pooled
        .sqr()
        .map_err(unavail)?
        .sum_keepdim(1)
        .map_err(unavail)?
        .sqrt()
        .map_err(unavail)?
        .clamp(1e-12, f32::MAX)
        .map_err(unavail)?;
    pooled
        .broadcast_div(&norm)
        .map_err(unavail)?
        .to_vec2::<f32>()
        .map_err(unavail)
}

fn unavail(error: impl ToString) -> EmbeddingError {
    EmbeddingError::Unavailable(error.to_string())
}
