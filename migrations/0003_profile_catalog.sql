-- Phase 2 profile graph: courses, preferences, availability, study lookups.

CREATE TABLE courses (
    id              uuid PRIMARY KEY,
    institution_id  uuid NOT NULL REFERENCES institutions (id) ON DELETE CASCADE,
    name            text NOT NULL,
    course_code     text,
    created_at      timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT courses_name_not_blank CHECK (btrim(name) <> ''),
    CONSTRAINT courses_code_not_blank CHECK (
        course_code IS NULL OR btrim(course_code) <> ''
    )
);

CREATE UNIQUE INDEX courses_institution_code_key
    ON courses (institution_id, course_code)
    WHERE course_code IS NOT NULL;

CREATE INDEX courses_institution_id_idx ON courses (institution_id);
CREATE INDEX courses_institution_name_lower_idx
    ON courses (institution_id, lower(name));

CREATE TABLE user_courses (
    user_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    course_id  uuid NOT NULL REFERENCES courses (id) ON DELETE RESTRICT,
    PRIMARY KEY (user_id, course_id)
);

CREATE TABLE user_preferences (
    user_id                     uuid PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    preferred_group_size_min    smallint NOT NULL,
    preferred_group_size_max    smallint NOT NULL,
    session_duration_minutes    smallint NOT NULL,
    CONSTRAINT user_preferences_size_min CHECK (preferred_group_size_min >= 2),
    CONSTRAINT user_preferences_size_max CHECK (preferred_group_size_max <= 50),
    CONSTRAINT user_preferences_size_order CHECK (
        preferred_group_size_max >= preferred_group_size_min
    ),
    CONSTRAINT user_preferences_duration CHECK (session_duration_minutes > 0)
);

CREATE TABLE availability (
    id           uuid PRIMARY KEY,
    user_id      uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    day_of_week  smallint NOT NULL,
    start_time   time NOT NULL,
    end_time     time NOT NULL,
    timezone     text NOT NULL,
    CONSTRAINT availability_day CHECK (day_of_week BETWEEN 0 AND 6),
    CONSTRAINT availability_window CHECK (end_time > start_time),
    CONSTRAINT availability_timezone_not_blank CHECK (btrim(timezone) <> '')
);

CREATE UNIQUE INDEX availability_slot_key
    ON availability (user_id, day_of_week, start_time, end_time, timezone);
CREATE INDEX availability_user_id_idx ON availability (user_id);

CREATE TABLE study_environments (
    id    uuid PRIMARY KEY,
    name  text NOT NULL,
    CONSTRAINT study_environments_name_not_blank CHECK (btrim(name) <> '')
);

CREATE UNIQUE INDEX study_environments_name_lower_key ON study_environments (lower(name));

CREATE TABLE study_methods (
    id    uuid PRIMARY KEY,
    name  text NOT NULL,
    CONSTRAINT study_methods_name_not_blank CHECK (btrim(name) <> '')
);

CREATE UNIQUE INDEX study_methods_name_lower_key ON study_methods (lower(name));

CREATE TABLE study_goals (
    id    uuid PRIMARY KEY,
    name  text NOT NULL,
    CONSTRAINT study_goals_name_not_blank CHECK (btrim(name) <> '')
);

CREATE UNIQUE INDEX study_goals_name_lower_key ON study_goals (lower(name));

CREATE TABLE user_study_environments (
    user_id         uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    environment_id  uuid NOT NULL REFERENCES study_environments (id) ON DELETE RESTRICT,
    PRIMARY KEY (user_id, environment_id)
);

CREATE TABLE user_study_methods (
    user_id    uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    method_id  uuid NOT NULL REFERENCES study_methods (id) ON DELETE RESTRICT,
    PRIMARY KEY (user_id, method_id)
);

CREATE TABLE user_study_goals (
    user_id  uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    goal_id  uuid NOT NULL REFERENCES study_goals (id) ON DELETE RESTRICT,
    PRIMARY KEY (user_id, goal_id)
);

-- Development seed values. Application logic looks these up by name.
INSERT INTO study_environments (id, name) VALUES
    ('01800000-0000-7000-8000-000000000001', 'Library'),
    ('01800000-0000-7000-8000-000000000002', 'Coffee shop'),
    ('01800000-0000-7000-8000-000000000003', 'Dorm'),
    ('01800000-0000-7000-8000-000000000004', 'Quiet room'),
    ('01800000-0000-7000-8000-000000000005', 'Online');

INSERT INTO study_methods (id, name) VALUES
    ('01800000-0000-7000-8000-000000000011', 'Practice problems'),
    ('01800000-0000-7000-8000-000000000012', 'Flashcards'),
    ('01800000-0000-7000-8000-000000000013', 'Group discussion'),
    ('01800000-0000-7000-8000-000000000014', 'Teaching others'),
    ('01800000-0000-7000-8000-000000000015', 'Reading notes');

INSERT INTO study_goals (id, name) VALUES
    ('01800000-0000-7000-8000-000000000021', 'Exam preparation'),
    ('01800000-0000-7000-8000-000000000022', 'Homework'),
    ('01800000-0000-7000-8000-000000000023', 'Project work'),
    ('01800000-0000-7000-8000-000000000024', 'Concept review');
