-- Phase 1: enable only extensions required by the approved schema.
-- UUIDs are generated in Rust (UUID v7); do not enable pgcrypto.

CREATE EXTENSION IF NOT EXISTS citext;
CREATE EXTENSION IF NOT EXISTS vector;
