-- Persistent group chat. PostgreSQL is the source of truth; WebSocket delivery is not.
--
-- Message body limit: 4000 Unicode characters (enforced in CHECK and in the service).
-- Idempotent sends: UNIQUE (group_id, sender_id, client_msg_id).
-- UUID v7 ids are time-ordered and used as history cursors.
-- message_attachments is reserved for a later media phase; this migration does not
-- implement uploads or object storage.

CREATE TABLE messages (
    id              UUID PRIMARY KEY,
    group_id        UUID NOT NULL REFERENCES study_groups (id) ON DELETE CASCADE,
    sender_id       UUID NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    client_msg_id   UUID NOT NULL,
    content         TEXT NOT NULL,
    message_type    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    edited_at       TIMESTAMPTZ,
    deleted_at      TIMESTAMPTZ,
    CONSTRAINT messages_type_check CHECK (
        message_type IN ('text', 'announcement', 'image', 'file')
    ),
    CONSTRAINT messages_content_length_check CHECK (char_length(content) <= 4000),
    CONSTRAINT messages_idempotency_key UNIQUE (group_id, sender_id, client_msg_id)
);

CREATE INDEX messages_group_created_idx
    ON messages (group_id, created_at DESC, id DESC);

CREATE INDEX messages_group_id_idx
    ON messages (group_id, id);

CREATE TABLE message_reactions (
    message_id  UUID NOT NULL REFERENCES messages (id) ON DELETE CASCADE,
    user_id     UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    emoji       TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (message_id, user_id, emoji),
    CONSTRAINT message_reactions_emoji_check CHECK (
        char_length(btrim(emoji)) BETWEEN 1 AND 32
    )
);

CREATE INDEX message_reactions_message_idx ON message_reactions (message_id);

-- Schema only. Do not write rows until the media phase.
CREATE TABLE message_attachments (
    id              UUID PRIMARY KEY,
    message_id      UUID NOT NULL REFERENCES messages (id) ON DELETE CASCADE,
    storage_key     TEXT NOT NULL,
    content_type    TEXT,
    byte_size       INTEGER,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT message_attachments_key_check CHECK (btrim(storage_key) <> ''),
    CONSTRAINT message_attachments_size_check CHECK (byte_size IS NULL OR byte_size > 0)
);

CREATE INDEX message_attachments_message_idx ON message_attachments (message_id);
