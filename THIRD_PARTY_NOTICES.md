# Third-party notices (embedding models)

StudySync does not vendor embedding weight files in this git repository. Operators must download pinned revisions from the upstream source at deploy time.

This product is intended for commercial/public release. Licenses below were read from the Hugging Face model cards (not from a copy of each full upstream repository). Confirm the license file at the pinned revision before redistribution.

## Selected production model

| Field | Value |
| --- | --- |
| Exact name | `Qwen/Qwen3-Embedding-0.6B` |
| Version id | `qwen3-embedding-0.6b:v1` |
| Revision | `c935f2d3fce3e2337b4a66ac3130faa6dada3218` |
| License | Apache License 2.0 |
| Source | https://huggingface.co/Qwen/Qwen3-Embedding-0.6B |
| Dimensions | 1024 (MRL 32–1024; StudySync stores 1024) |
| Context | 32k tokens (model card) |
| Notice | Redistribution of Apache-2.0 materials requires retaining copyright, license, and NOTICE files from the upstream package. This repository does not copy those files; include them beside downloaded weights at deploy time. |

## Fallback model (not the production default)

| Field | Value |
| --- | --- |
| Exact name | `sentence-transformers/all-MiniLM-L6-v2` |
| Version id | `all-minilm-l6-v2:legacy` |
| Revision | `8b3219a92973c328a8e22fadcfa821b5dc75636a` |
| License | Apache-2.0 (declared on the Hugging Face model card) |
| Source | https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2 |
| Dimensions | 384 |
| Role | Legacy StudySync baseline; operational fallback if Qwen weights cannot be hosted |

## Evaluated but not selected

### `BAAI/bge-m3`

- Revision recorded from the model card tree: `6892b95fed65c899a30896eb40d619ae284d0455`
- License: MIT
- Dimensions: 1024
- Source: https://huggingface.co/BAAI/bge-m3
- Not selected: multilingual dense/sparse/ColBERT path is more operationally complex than StudySync needs for an English-first campus product; StudySync-specific ranking did not justify the extra deployment surface.

### `intfloat/multilingual-e5-large-instruct`

- Revision recorded from a documented model-card commit: `b9fbfc1b8c70709e5371d98135d214b005562b7a`
- License: MIT
- Dimensions: 1024
- Source: https://huggingface.co/intfloat/multilingual-e5-large-instruct
- Not selected: 512-token context is tighter than Qwen; similar size to BGE with less operational upside for StudySync after the frozen eval.

## Runtime (Candle)

The Qwen3 decoder used at inference time is a text-only Candle port adapted from Anush008/fastembed-rs (`qwen3` feature, Apache-2.0). This repository does not depend on `fastembed` or ONNX Runtime.

Production loads the official Qwen tokenizer and safetensors via Hugging Face Hub into `EMBEDDING_CACHE_DIR`. Weights are not in git. Runtime: in-process Candle Qwen3 transformer on CPU (F32), 1024-d L2-normalized last-token pooling, instruct-formatted queries. ONNX Runtime is not used.

The hashed n-gram encoder remains a **unit-test-only** stand-in. It is not selectable as `EMBEDDING_BACKEND` and must not be treated as Qwen quality evidence. Historical stand-in numbers are labeled in `eval/RESULTS.md`.
