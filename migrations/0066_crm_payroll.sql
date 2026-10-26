-- CRM payroll: staff pay rates and payouts (kinds 42160–42162).
--
-- `crm_staff_pay` holds the current rates of each staff member; accruals are
-- computed from the journal when read, so a rate change applies to every
-- period read afterwards. `crm_staff_payouts` records money handed over; a
-- cancelled payout keeps its row with `voided_at` set.
CREATE TABLE crm_staff_pay (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    staff_id           UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    percent_bp         INTEGER     NOT NULL DEFAULT 0 CHECK (percent_bp BETWEEN 0 AND 10000),
    per_visit_minor    BIGINT      NOT NULL DEFAULT 0 CHECK (per_visit_minor BETWEEN 0 AND 100000000000),
    shift_minor        BIGINT      NOT NULL DEFAULT 0 CHECK (shift_minor BETWEEN 0 AND 100000000000),
    revision_event_id  BYTEA       NOT NULL CHECK (length(revision_event_id) = 32),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, staff_id),
    FOREIGN KEY (community_id, staff_id) REFERENCES crm_staff (community_id, staff_id)
);

CREATE TABLE crm_staff_payouts (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    payout_id          UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    staff_id           UUID        NOT NULL,
    day                DATE        NOT NULL,
    amount_minor       BIGINT      NOT NULL CHECK (amount_minor BETWEEN 1 AND 100000000000),
    note               TEXT        CHECK (note IS NULL OR char_length(note) BETWEEN 1 AND 256),
    event_id           BYTEA       NOT NULL CHECK (length(event_id) = 32),
    created_by         BYTEA       NOT NULL CHECK (length(created_by) = 32),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    voided_at          TIMESTAMPTZ,
    void_event_id      BYTEA       CHECK (void_event_id IS NULL OR length(void_event_id) = 32),
    PRIMARY KEY (community_id, payout_id),
    FOREIGN KEY (community_id, staff_id) REFERENCES crm_staff (community_id, staff_id),
    CHECK ((voided_at IS NULL) = (void_event_id IS NULL))
);

CREATE INDEX crm_staff_payouts_by_day
    ON crm_staff_payouts (community_id, space_channel_id, day);

SELECT attach_community_write_fence('crm_staff_pay');
SELECT attach_community_write_fence('crm_staff_payouts');
