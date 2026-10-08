#!/usr/bin/env python3
"""Development-only Hugging Face reference for nomic-embed-text-v1.5.

Not a production backend. Pins the same Hub revision as the Rust catalog.
Pooling matches the model card: mean pool, F.layer_norm (eps=1e-5), L2.
"""

from __future__ import annotations

import json
import os
from pathlib import Path

import torch
import torch.nn.functional as F
from transformers import AutoModel, BertTokenizerFast

REVISION = "e9b6763023c676ca8431644204f50c2b100d9aab"
REPO = "nomic-ai/nomic-embed-text-v1.5"
QUERY_PREFIX = "search_query: "
DOCUMENT_PREFIX = "search_document: "

CASES = [
    ("query", "short_query", "networks study group"),
    ("query", "related_query", "calculus exam prep"),
    ("query", "unrelated_query", "organic chemistry lab"),
    (
        "document",
        "study_group",
        "Weekly Computer Networks homework help and exam prep for CS majors.",
    ),
    (
        "document",
        "related_doc",
        "Calculus I study group. We work WebAssign problem sets and review derivatives before exams.",
    ),
    (
        "document",
        "unrelated_doc",
        "Organic Chemistry exam prep. Mechanism practice, NMR interpretation, and lab report review.",
    ),
    (
        "document",
        "long_doc",
        (
            "This group meets twice a week in the engineering library for computer networks. "
            "Topics include TCP/IP, routing, congestion control, socket programming, and Wireshark labs. "
            "Members are CS and CE undergraduates preparing for midterms. We also cover subnetting, "
            "DNS, HTTP, and a short unit on wireless. Bring a laptop and the Kurose Ross textbook. "
            "The session runs two hours with a short break. New members are welcome if they have "
            "completed the intro systems course."
        ),
    ),
]


def snapshot_dir() -> Path:
    cache = Path(os.environ.get("EMBEDDING_CACHE_DIR", ".fastembed_cache"))
    path = cache / f"models--{REPO.replace('/', '--')}" / "snapshots" / REVISION
    if not path.is_dir():
        raise SystemExit(f"missing pinned snapshot {path}")
    return path


def mean_pooling(token_embeddings: torch.Tensor, attention_mask: torch.Tensor) -> torch.Tensor:
    mask = attention_mask.unsqueeze(-1).expand(token_embeddings.size()).float()
    return torch.sum(token_embeddings * mask, 1) / torch.clamp(mask.sum(1), min=1e-9)


def embed(model, tokenizer, texts: list[str]) -> tuple[list[list[int]], list[list[int]], torch.Tensor]:
    encoded = tokenizer(
        texts,
        padding=True,
        truncation=True,
        max_length=512,
        return_tensors="pt",
    )
    with torch.no_grad():
        output = model(**encoded)
    pooled = mean_pooling(output[0], encoded["attention_mask"])
    pooled = F.layer_norm(pooled, normalized_shape=(pooled.shape[1],))
    pooled = F.normalize(pooled, p=2, dim=1)
    return (
        encoded["input_ids"].tolist(),
        encoded["attention_mask"].tolist(),
        pooled,
    )


def cosine(a: torch.Tensor, b: torch.Tensor) -> float:
    return float(torch.dot(a, b).item())


def main() -> None:
    path = snapshot_dir()
    tokenizer = BertTokenizerFast.from_pretrained(str(path))
    model = AutoModel.from_pretrained(str(path), trust_remote_code=True)
    model.eval()

    prefixed = []
    meta = []
    for role, name, text in CASES:
        prefix = QUERY_PREFIX if role == "query" else DOCUMENT_PREFIX
        prefixed.append(prefix + text)
        meta.append({"role": role, "name": name, "text": text, "prefixed": prefix + text})

    ids, masks, vectors = embed(model, tokenizer, prefixed)
    records = []
    for item, tok_ids, mask, vec in zip(meta, ids, masks, vectors):
        records.append(
            {
                **item,
                "input_ids": tok_ids,
                "attention_mask": mask,
                "dim": int(vec.numel()),
                "l2": float(torch.linalg.vector_norm(vec).item()),
                "embedding": [float(x) for x in vec.tolist()],
            }
        )

    by_name = {row["name"]: i for i, row in enumerate(records)}
    pairs = [
        ("related_query", "related_doc"),
        ("related_query", "unrelated_doc"),
        ("short_query", "study_group"),
        ("short_query", "unrelated_doc"),
        ("unrelated_query", "unrelated_doc"),
        ("related_query", "long_doc"),
    ]
    similarities = []
    for left, right in pairs:
        similarities.append(
            {
                "left": left,
                "right": right,
                "cosine": cosine(vectors[by_name[left]], vectors[by_name[right]]),
            }
        )

    payload = {
        "repo": REPO,
        "revision": REVISION,
        "tokenizer": "BertTokenizerFast + tokenizer.json [CLS]/[SEP]/[PAD]",
        "prefixes": {"query": QUERY_PREFIX, "document": DOCUMENT_PREFIX},
        "pooling": "attention-mask mean, F.layer_norm eps=1e-5, L2",
        "max_length": 512,
        "special_tokens": {
            "cls": tokenizer.cls_token_id,
            "sep": tokenizer.sep_token_id,
            "pad": tokenizer.pad_token_id,
            "unk": tokenizer.unk_token_id,
        },
        "records": records,
        "similarities": similarities,
    }
    out = Path(__file__).with_name("nomic_hf_reference.json")
    out.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(f"wrote {out} dim={records[0]['dim']} n={len(records)}")
    for row in similarities:
        print(f"cos {row['left']} vs {row['right']}: {row['cosine']:.6f}")


if __name__ == "__main__":
    main()
