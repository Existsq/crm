-- Phone verification for the public booking page.
--
-- A client asks for a code, gets it by SMS and books with it. Only an HMAC of
-- the code is kept. A code expires after `expires_at`, allows a few wrong
-- attempts and books one appointment (`used_at`, `appointment_id`). Rows are
-- also the per-phone and per-community send counters, so the relay deletes
-- them a day after they were issued.
CREATE TABLE crm_booking_codes (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    verification_id    UUID        NOT NULL,
    phone              TEXT        NOT NULL CHECK (char_length(phone) BETWEEN 1 AND 32),
    -- Digits with the Russian trunk 8 normalized to 7.
    phone_key          TEXT        NOT NULL CHECK (phone_key ~ '^[0-9]{5,15}$'),
    code_hash          BYTEA       NOT NULL CHECK (length(code_hash) = 32),
    attempts           INTEGER     NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at         TIMESTAMPTZ NOT NULL,
    verified_at        TIMESTAMPTZ,
    used_at            TIMESTAMPTZ,
    appointment_id     UUID,
    PRIMARY KEY (community_id, verification_id),
    CHECK (expires_at > created_at),
    CHECK (used_at IS NULL OR (verified_at IS NOT NULL AND appointment_id IS NOT NULL))
);

CREATE INDEX crm_booking_codes_by_phone
    ON crm_booking_codes (community_id, phone_key, created_at);
CREATE INDEX crm_booking_codes_by_time
    ON crm_booking_codes (community_id, created_at);

SELECT attach_community_write_fence('crm_booking_codes');
