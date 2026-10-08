-- Canonical media metadata. Existing message_attachments / direct_message_attachments
-- stay as Phase 5 placeholders (message_id NOT NULL, no staging). Phase 6 writes
-- only to media_objects so uploads can exist before a message is created.
--
-- Ownership model:
--   uploaded  = object stored, not yet referenced by a message/profile
--   attached  = referenced by a message or current profile picture
--   deleted   = tombstone; object should be removed from storage
--
-- Orphans (status = uploaded AND created_at older than 24h) are removed by
-- services::media::cleanup_orphaned_media. Production should run that on a
-- schedule (cron / later Redis-backed worker). Account deletion should call
-- the same cleanup for that owner. Message deletion marks rows deleted and
-- best-effort-deletes objects; moderation holds are not implemented.

CREATE TABLE media_objects (
    id                  UUID PRIMARY KEY,
    owner_id            UUID NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    purpose             TEXT NOT NULL,
    kind                TEXT NOT NULL,
    status              TEXT NOT NULL,
    storage_key         TEXT NOT NULL,
    file_name           TEXT NOT NULL,
    content_type        TEXT NOT NULL,
    byte_size           INTEGER NOT NULL,
    group_id            UUID REFERENCES study_groups (id) ON DELETE CASCADE,
    peer_id             UUID REFERENCES users (id) ON DELETE RESTRICT,
    message_id          UUID REFERENCES messages (id) ON DELETE SET NULL,
    direct_message_id   UUID REFERENCES direct_messages (id) ON DELETE SET NULL,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    attached_at         TIMESTAMPTZ,
    deleted_at          TIMESTAMPTZ,
    CONSTRAINT media_objects_purpose_check CHECK (
        purpose IN ('profile', 'group_message', 'direct_message')
    ),
    CONSTRAINT media_objects_kind_check CHECK (kind IN ('image', 'file')),
    CONSTRAINT media_objects_status_check CHECK (status IN ('uploaded', 'attached', 'deleted')),
    CONSTRAINT media_objects_key_check CHECK (btrim(storage_key) <> ''),
    CONSTRAINT media_objects_file_name_check CHECK (btrim(file_name) <> ''),
    CONSTRAINT media_objects_type_check CHECK (btrim(content_type) <> ''),
    CONSTRAINT media_objects_size_check CHECK (byte_size > 0),
    CONSTRAINT media_objects_storage_key_key UNIQUE (storage_key),
    CONSTRAINT media_objects_group_purpose CHECK (
        (purpose = 'group_message' AND group_id IS NOT NULL)
        OR (purpose <> 'group_message' AND group_id IS NULL)
    ),
    CONSTRAINT media_objects_dm_purpose CHECK (
        (purpose = 'direct_message' AND peer_id IS NOT NULL)
        OR (purpose <> 'direct_message' AND peer_id IS NULL)
    ),
    CONSTRAINT media_objects_profile_kind CHECK (
        purpose <> 'profile' OR kind = 'image'
    )
);

CREATE INDEX media_objects_owner_status_idx ON media_objects (owner_id, status);
CREATE INDEX media_objects_message_idx ON media_objects (message_id)
    WHERE message_id IS NOT NULL;
CREATE INDEX media_objects_dm_idx ON media_objects (direct_message_id)
    WHERE direct_message_id IS NOT NULL;
CREATE INDEX media_objects_group_idx ON media_objects (group_id)
    WHERE group_id IS NOT NULL;
CREATE INDEX media_objects_orphan_idx ON media_objects (created_at)
    WHERE status = 'uploaded';
