-- Phase 8G.2: FCM device registrations, per-user push preferences, and a
-- transactional outbox so in-app notifications remain the source of truth.

CREATE TABLE push_devices (
    id                  uuid PRIMARY KEY,
    user_id             uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    platform            text NOT NULL,
    fcm_token           text NOT NULL,
    token_fingerprint   text NOT NULL,
    installation_id     text,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    last_seen_at        timestamptz NOT NULL DEFAULT now(),
    revoked_at          timestamptz,
    CONSTRAINT push_devices_platform_valid CHECK (platform IN ('android', 'ios')),
    CONSTRAINT push_devices_token_not_blank CHECK (btrim(fcm_token) <> ''),
    CONSTRAINT push_devices_fingerprint_sha256 CHECK (token_fingerprint ~ '^[0-9a-f]{64}$')
);

CREATE UNIQUE INDEX push_devices_active_fingerprint_idx
    ON push_devices (token_fingerprint)
    WHERE revoked_at IS NULL;

CREATE UNIQUE INDEX push_devices_active_installation_idx
    ON push_devices (user_id, installation_id)
    WHERE revoked_at IS NULL AND installation_id IS NOT NULL;

CREATE INDEX push_devices_user_active_idx
    ON push_devices (user_id)
    WHERE revoked_at IS NULL;

CREATE TABLE push_preferences (
    user_id         uuid PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    push_enabled    boolean NOT NULL DEFAULT true,
    friends         boolean NOT NULL DEFAULT true,
    groups          boolean NOT NULL DEFAULT true,
    messages        boolean NOT NULL DEFAULT true,
    announcements   boolean NOT NULL DEFAULT true,
    updated_at      timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE push_outbox (
    id                  uuid PRIMARY KEY,
    notification_id     uuid NOT NULL REFERENCES notifications (id) ON DELETE CASCADE,
    user_id             uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status              text NOT NULL DEFAULT 'pending',
    attempts            integer NOT NULL DEFAULT 0,
    next_attempt_at     timestamptz NOT NULL DEFAULT now(),
    last_error_class    text,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    delivered_at        timestamptz,
    CONSTRAINT push_outbox_status_valid CHECK (
        status IN ('pending', 'sending', 'delivered', 'skipped', 'failed')
    ),
    CONSTRAINT push_outbox_attempts_nonneg CHECK (attempts >= 0),
    CONSTRAINT push_outbox_notification_unique UNIQUE (notification_id)
);

CREATE INDEX push_outbox_due_idx
    ON push_outbox (next_attempt_at, created_at)
    WHERE status = 'pending';
