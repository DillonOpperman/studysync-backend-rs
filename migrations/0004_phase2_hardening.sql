-- Phase 2 hardening: per-institution course names, reverse lookup indexes,
-- overnight availability windows (end_time <> start_time; end < start means wrap).

DROP INDEX IF EXISTS courses_institution_name_lower_idx;

CREATE UNIQUE INDEX courses_institution_name_lower_key
    ON courses (institution_id, lower(name));

CREATE INDEX user_courses_course_id_idx ON user_courses (course_id);

-- Weekday + start clock for future overlap matching. Per-user slots stay on
-- availability_user_id_idx / availability_slot_key.
CREATE INDEX availability_day_start_idx ON availability (day_of_week, start_time);

ALTER TABLE availability DROP CONSTRAINT availability_window;
ALTER TABLE availability ADD CONSTRAINT availability_nonzero_window
    CHECK (end_time <> start_time);
