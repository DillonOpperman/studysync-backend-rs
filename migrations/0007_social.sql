-- Phase 4: blocks, friendships, friend requests, message requests, DMs.
-- Friendship is a single undirected pair (user_low, user_high) with user_low < user_high
-- so A→B and B→A cannot become two accepted friendships.

CREATE TABLE user_blocks (
    blocker_id  uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    blocked_id  uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (blocker_id, blocked_id),
    CONSTRAINT user_blocks_not_self CHECK (blocker_id <> blocked_id)
);

CREATE INDEX user_blocks_blocked_id_idx ON user_blocks (blocked_id);

CREATE TABLE friendships (
    user_low    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    user_high   uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_low, user_high),
    CONSTRAINT friendships_ordered CHECK (user_low < user_high)
);

CREATE INDEX friendships_user_high_idx ON friendships (user_high);

CREATE TABLE friend_requests (
    id             uuid PRIMARY KEY,
    requester_id   uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    recipient_id   uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status         text NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    decided_at     timestamptz,
    CONSTRAINT friend_requests_not_self CHECK (requester_id <> recipient_id),
    CONSTRAINT friend_requests_status_valid CHECK (
        status IN ('pending', 'accepted', 'rejected')
    ),
    CONSTRAINT friend_requests_decided_at_consistent CHECK (
        (status = 'pending' AND decided_at IS NULL)
        OR (status IN ('accepted', 'rejected') AND decided_at IS NOT NULL)
    )
);

-- One pending request per unordered pair (covers A→B and B→A).
CREATE UNIQUE INDEX friend_requests_one_pending
    ON friend_requests (
        LEAST(requester_id, recipient_id),
        GREATEST(requester_id, recipient_id)
    )
    WHERE status = 'pending';
CREATE INDEX friend_requests_recipient_status_idx
    ON friend_requests (recipient_id, status);
CREATE INDEX friend_requests_requester_status_idx
    ON friend_requests (requester_id, status);

CREATE TABLE message_requests (
    id           uuid PRIMARY KEY,
    sender_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    receiver_id  uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status       text NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT message_requests_not_self CHECK (sender_id <> receiver_id),
    CONSTRAINT message_requests_status_valid CHECK (
        status IN ('pending', 'accepted', 'rejected')
    )
);

CREATE UNIQUE INDEX message_requests_one_pending
    ON message_requests (sender_id, receiver_id)
    WHERE status = 'pending';
CREATE INDEX message_requests_receiver_status_idx
    ON message_requests (receiver_id, status);
CREATE INDEX message_requests_sender_status_idx
    ON message_requests (sender_id, status);

CREATE TABLE direct_messages (
    id            uuid PRIMARY KEY,
    sender_id     uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    receiver_id   uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    content       text NOT NULL,
    message_type  text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    read_at       timestamptz,
    CONSTRAINT direct_messages_not_self CHECK (sender_id <> receiver_id),
    CONSTRAINT direct_messages_type_valid CHECK (
        message_type IN ('text', 'image', 'file')
    ),
    CONSTRAINT direct_messages_content_not_blank CHECK (btrim(content) <> '')
);

CREATE INDEX direct_messages_thread_idx ON direct_messages (
    LEAST(sender_id, receiver_id),
    GREATEST(sender_id, receiver_id),
    created_at
);
CREATE INDEX direct_messages_unread_idx
    ON direct_messages (receiver_id, sender_id)
    WHERE read_at IS NULL;

-- Object-storage keys only; media upload is a later phase.
CREATE TABLE direct_message_attachments (
    id            uuid PRIMARY KEY,
    message_id    uuid NOT NULL REFERENCES direct_messages (id) ON DELETE CASCADE,
    storage_key   text NOT NULL,
    content_type  text NOT NULL DEFAULT '',
    byte_size     integer,
    created_at    timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT direct_message_attachments_key_not_blank CHECK (btrim(storage_key) <> '')
);

CREATE INDEX direct_message_attachments_message_id_idx
    ON direct_message_attachments (message_id);
