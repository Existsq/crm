-- CRM booking: service durations, staff and their working days
-- (kinds 42102, 42131, 42132).
--
-- Services are products whose fiscal type is a service or work; a duration
-- makes them bookable. `crm_staff` holds the current revision of each staff
-- member; `crm_staff_days` the working intervals of each calendar day, as
-- minutes after midnight in the space timezone. A missing day is not
-- scheduled; a row with no intervals is a day off.
ALTER TABLE crm_products
    ADD COLUMN duration_min INTEGER
        CHECK (duration_min IS NULL OR (duration_min BETWEEN 5 AND 720 AND duration_min % 5 = 0));

CREATE TABLE crm_staff (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    staff_id           UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    name               TEXT        NOT NULL CHECK (char_length(name) BETWEEN 1 AND 128),
    position           TEXT        CHECK (position IS NULL OR char_length(position) BETWEEN 1 AND 64),
    phone              TEXT        CHECK (phone IS NULL OR char_length(phone) BETWEEN 1 AND 32),
    service_ids        UUID[]      NOT NULL DEFAULT '{}' CHECK (cardinality(service_ids) <= 200),
    member_pubkey      BYTEA       CHECK (member_pubkey IS NULL OR length(member_pubkey) = 32),
    archived_at        TIMESTAMPTZ,
    revision_event_id  BYTEA       NOT NULL CHECK (length(revision_event_id) = 32),
    created_by         BYTEA       NOT NULL CHECK (length(created_by) = 32),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, staff_id),
    FOREIGN KEY (community_id, space_channel_id) REFERENCES crm_spaces (community_id, channel_id)
);

CREATE INDEX crm_staff_by_name
    ON crm_staff (community_id, space_channel_id, lower(name), staff_id);

CREATE TABLE crm_staff_days (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    staff_id           UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    day                DATE        NOT NULL,
    -- [{"start": 600, "end": 1140}, …] in minutes, sorted, non-overlapping.
    intervals          JSONB       NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(intervals) = 'array'),
    revision_event_id  BYTEA       NOT NULL CHECK (length(revision_event_id) = 32),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, staff_id, day),
    FOREIGN KEY (community_id, staff_id) REFERENCES crm_staff (community_id, staff_id)
);

CREATE INDEX crm_staff_days_by_day
    ON crm_staff_days (community_id, space_channel_id, day);

SELECT attach_community_write_fence('crm_staff');
SELECT attach_community_write_fence('crm_staff_days');
