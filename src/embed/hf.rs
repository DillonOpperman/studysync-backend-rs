use std::path::{Path, PathBuf};

use hf_hub::api::sync::ApiBuilder;
use hf_hub::{Repo, RepoType};

use super::EmbeddingError;

pub fn snapshot_file(
    cache_dir: &Path,
    repo_id: &str,
    revision: &str,
    filename: &str,
) -> Result<PathBuf, EmbeddingError> {
    std::fs::create_dir_all(cache_dir).map_err(|error| {
        EmbeddingError::Unavailable(format!("cannot create embedding cache: {error}"))
    })?;
    let api = ApiBuilder::new()
        .with_cache_dir(cache_dir.to_path_buf())
        .with_progress(true)
        .build()
        .map_err(|error| EmbeddingError::Unavailable(error.to_string()))?;
    let repo = api.repo(Repo::with_revision(
        repo_id.to_string(),
        RepoType::Model,
        revision.to_string(),
    ));
    let path = repo
        .get(filename)
        .map_err(|error| EmbeddingError::Unavailable(format!("{repo_id} {filename}: {error}")))?;
    verify_snapshot(&path, revision)?;
    Ok(path)
}

pub fn verify_snapshot(config_or_weight: &Path, revision: &str) -> Result<(), EmbeddingError> {
    let name = config_or_weight
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if name != revision {
        return Err(EmbeddingError::Unavailable(format!(
            "refusing unpinned snapshot {name}; expected {revision}"
        )));
    }
    Ok(())
}

pub fn weight_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}
