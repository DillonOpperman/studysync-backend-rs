-- Phase 3 notifications. JSONB `data` is event/deep-link payload only.

CREATE TABLE notifications (
    id          uuid PRIMARY KEY,
    user_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    type        text NOT NULL,
    title       text NOT NULL,
    body        text NOT NULL,
    data        jsonb NOT NULL DEFAULT '{}'::jsonb,
    read_at     timestamptz,
    created_at  timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT notifications_type_not_blank CHECK (btrim(type) <> ''),
    CONSTRAINT notifications_title_not_blank CHECK (btrim(title) <> '')
);

CREATE INDEX notifications_user_created_idx
    ON notifications (user_id, created_at DESC);
CREATE INDEX notifications_user_unread_idx
    ON notifications (user_id)
    WHERE read_at IS NULL;
