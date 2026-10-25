-- CRM customers and suppliers (kind 42103, docs/crm/adr/0001).
--
-- `crm_parties` holds the current revision of each counterparty; the signed
-- kind 42103 events are the history. An INN identifies one counterparty per
-- space, archived ones included (sales reference parties forever).
CREATE TABLE crm_parties (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    party_id           UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    party_type         TEXT        NOT NULL CHECK (party_type IN ('customer', 'supplier', 'both')),
    name               TEXT        NOT NULL CHECK (char_length(name) BETWEEN 1 AND 128),
    phone              TEXT        CHECK (phone IS NULL OR char_length(phone) BETWEEN 1 AND 32),
    -- Digits of `phone`, for search by any part of the number.
    phone_digits       TEXT        CHECK (phone_digits IS NULL OR phone_digits ~ '^[0-9]{5,15}$'),
    email              TEXT        CHECK (email IS NULL OR char_length(email) BETWEEN 3 AND 254),
    inn                TEXT        CHECK (inn IS NULL OR inn ~ '^([0-9]{10}|[0-9]{12})$'),
    address            TEXT        CHECK (address IS NULL OR char_length(address) BETWEEN 1 AND 256),
    note               TEXT        CHECK (note IS NULL OR char_length(note) BETWEEN 1 AND 2000),
    archived_at        TIMESTAMPTZ,
    revision_event_id  BYTEA       NOT NULL CHECK (length(revision_event_id) = 32),
    created_by         BYTEA       NOT NULL CHECK (length(created_by) = 32),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, party_id),
    FOREIGN KEY (community_id, space_channel_id) REFERENCES crm_spaces (community_id, channel_id)
);

CREATE UNIQUE INDEX crm_parties_inn_unique
    ON crm_parties (community_id, space_channel_id, inn) WHERE inn IS NOT NULL;

-- Keyset order of GET /crm/parties.
CREATE INDEX crm_parties_by_name
    ON crm_parties (community_id, space_channel_id, lower(name), party_id);

SELECT attach_community_write_fence('crm_parties');
