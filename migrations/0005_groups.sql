-- Phase 3: study groups, recurring slots, memberships, join requests.
-- Notification tables are in 0006 (0004 is Phase 2 hardening).

CREATE TABLE study_groups (
    id           uuid PRIMARY KEY,
    title        text NOT NULL,
    subject      text NOT NULL,
    course_id    uuid REFERENCES courses (id) ON DELETE SET NULL,
    description  text NOT NULL DEFAULT '',
    location     text NOT NULL DEFAULT '',
    max_members  smallint NOT NULL,
    leader_id    uuid NOT NULL REFERENCES users (id) ON DELETE RESTRICT,
    status       text NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT study_groups_title_not_blank CHECK (btrim(title) <> ''),
    CONSTRAINT study_groups_subject_not_blank CHECK (btrim(subject) <> ''),
    CONSTRAINT study_groups_max_members_range CHECK (max_members BETWEEN 2 AND 50),
    CONSTRAINT study_groups_status_valid CHECK (status IN ('active', 'archived'))
);

CREATE INDEX study_groups_leader_id_idx ON study_groups (leader_id);
CREATE INDEX study_groups_status_idx ON study_groups (status);
CREATE INDEX study_groups_course_id_idx ON study_groups (course_id)
    WHERE course_id IS NOT NULL;

-- Recurring weekly slots. end_time < start_time means overnight (wrap past midnight).
-- Matching must expand overnight rows into [start, 24:00) on day_of_week and
-- [00:00, end) on the following day. Equal start/end is invalid.
CREATE TABLE group_recurring_slots (
    id           uuid PRIMARY KEY,
    group_id     uuid NOT NULL REFERENCES study_groups (id) ON DELETE CASCADE,
    day_of_week  smallint NOT NULL,
    start_time   time NOT NULL,
    end_time     time NOT NULL,
    timezone     text NOT NULL,
    CONSTRAINT group_recurring_slots_day CHECK (day_of_week BETWEEN 0 AND 6),
    CONSTRAINT group_recurring_slots_nonzero CHECK (end_time <> start_time),
    CONSTRAINT group_recurring_slots_timezone_not_blank CHECK (btrim(timezone) <> '')
);

CREATE INDEX group_recurring_slots_group_id_idx ON group_recurring_slots (group_id);
CREATE UNIQUE INDEX group_recurring_slots_unique
    ON group_recurring_slots (group_id, day_of_week, start_time, end_time, timezone);

CREATE TABLE group_members (
    id         uuid PRIMARY KEY,
    group_id   uuid NOT NULL REFERENCES study_groups (id) ON DELETE CASCADE,
    user_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role       text NOT NULL,
    status     text NOT NULL,
    joined_at  timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT group_members_role_valid CHECK (role IN ('leader', 'member')),
    CONSTRAINT group_members_status_valid CHECK (status IN ('active', 'archived'))
);

CREATE UNIQUE INDEX group_members_user_group_key ON group_members (group_id, user_id);
CREATE UNIQUE INDEX group_members_one_active_leader
    ON group_members (group_id)
    WHERE role = 'leader' AND status = 'active';
CREATE INDEX group_members_group_status_idx ON group_members (group_id, status);
CREATE INDEX group_members_user_status_idx ON group_members (user_id, status);

CREATE TABLE group_join_requests (
    id            uuid PRIMARY KEY,
    group_id      uuid NOT NULL REFERENCES study_groups (id) ON DELETE CASCADE,
    requester_id  uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    status        text NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now(),
    decided_at    timestamptz,
    CONSTRAINT group_join_requests_status_valid CHECK (
        status IN ('pending', 'approved', 'rejected')
    ),
    CONSTRAINT group_join_requests_decided_at_consistent CHECK (
        (status = 'pending' AND decided_at IS NULL)
        OR (status IN ('approved', 'rejected') AND decided_at IS NOT NULL)
    )
);

CREATE UNIQUE INDEX group_join_requests_one_pending
    ON group_join_requests (group_id, requester_id)
    WHERE status = 'pending';
CREATE INDEX group_join_requests_group_status_idx
    ON group_join_requests (group_id, status);
CREATE INDEX group_join_requests_requester_status_idx
    ON group_join_requests (requester_id, status);
