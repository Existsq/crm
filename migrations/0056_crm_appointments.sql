-- CRM appointments (kind 42133): a client booked with one staff member.
--
-- `day`, `start_min` and `end_min` are the visit's calendar date and minutes
-- after midnight in the space timezone. Bookings that hold their time
-- (`status` other than no_show/cancelled) never overlap for one staff member:
-- the writer locks the staff row and checks before inserting. `sale_id` is the
-- sale that paid for the visit.
CREATE TABLE crm_appointments (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    appointment_id     UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    staff_id           UUID        NOT NULL,
    day                DATE        NOT NULL,
    start_min          INTEGER     NOT NULL CHECK (start_min BETWEEN 0 AND 1435),
    end_min            INTEGER     NOT NULL CHECK (end_min BETWEEN 5 AND 1440 AND end_min > start_min),
    -- [{"product_id", "name", "duration_min", "price_minor"}, …] in order.
    services           JSONB       NOT NULL CHECK (jsonb_typeof(services) = 'array'),
    total_minor        BIGINT      NOT NULL CHECK (total_minor >= 0),
    customer_id        UUID,
    client_name        TEXT        CHECK (client_name IS NULL OR char_length(client_name) BETWEEN 1 AND 128),
    client_phone       TEXT        CHECK (client_phone IS NULL OR char_length(client_phone) BETWEEN 1 AND 32),
    status             TEXT        NOT NULL CHECK (status IN ('booked', 'confirmed', 'arrived', 'done', 'no_show', 'cancelled')),
    note               TEXT        CHECK (note IS NULL OR char_length(note) BETWEEN 1 AND 500),
    source             TEXT        NOT NULL DEFAULT 'staff' CHECK (source IN ('staff', 'online')),
    outside_hours      BOOLEAN     NOT NULL DEFAULT false,
    sale_id            UUID,
    revision_event_id  BYTEA       NOT NULL CHECK (length(revision_event_id) = 32),
    created_by         BYTEA       NOT NULL CHECK (length(created_by) = 32),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, appointment_id),
    FOREIGN KEY (community_id, staff_id) REFERENCES crm_staff (community_id, staff_id),
    FOREIGN KEY (community_id, customer_id) REFERENCES crm_parties (community_id, party_id),
    CHECK (customer_id IS NOT NULL OR client_name IS NOT NULL)
);

CREATE INDEX crm_appointments_by_day
    ON crm_appointments (community_id, space_channel_id, day, start_min);
CREATE INDEX crm_appointments_by_staff_day
    ON crm_appointments (community_id, staff_id, day);
CREATE INDEX crm_appointments_by_customer
    ON crm_appointments (community_id, customer_id, day) WHERE customer_id IS NOT NULL;
CREATE UNIQUE INDEX crm_appointments_sale_unique
    ON crm_appointments (community_id, sale_id) WHERE sale_id IS NOT NULL;

SELECT attach_community_write_fence('crm_appointments');
