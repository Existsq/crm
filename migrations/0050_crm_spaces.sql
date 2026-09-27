-- CRM spaces and warehouses (docs/crm/adr/0001, 0002).
--
-- A CRM space binds one private channel per community as the retail
-- workspace: its members read CRM data, their channel role decides writes.
-- Rows are projections of signed CRM events (kinds 42100/42101) written in the
-- same transaction as the event; `revision_event_id` is the id of the event
-- that produced the current row and serves as the `expected-revision` CAS key.
CREATE TABLE crm_spaces (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    channel_id         UUID        NOT NULL,
    settings           JSONB       NOT NULL CHECK (jsonb_typeof(settings) = 'object'),
    revision_event_id  BYTEA       NOT NULL CHECK (length(revision_event_id) = 32),
    created_by         BYTEA       NOT NULL CHECK (length(created_by) = 32),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, channel_id),
    FOREIGN KEY (community_id, channel_id) REFERENCES channels (community_id, id)
);

-- One CRM space per community.
CREATE UNIQUE INDEX crm_spaces_one_per_community ON crm_spaces (community_id);

CREATE TABLE crm_warehouses (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    warehouse_id       UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    name               TEXT        NOT NULL CHECK (char_length(name) BETWEEN 1 AND 128),
    is_default         BOOLEAN     NOT NULL DEFAULT FALSE,
    archived_at        TIMESTAMPTZ,
    revision_event_id  BYTEA       NOT NULL CHECK (length(revision_event_id) = 32),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, warehouse_id),
    FOREIGN KEY (community_id, space_channel_id) REFERENCES crm_spaces (community_id, channel_id),
    CHECK (NOT (is_default AND archived_at IS NOT NULL))
);

-- Exactly one default warehouse per space is enforced by this index plus the
-- relay (which moves the flag inside the write transaction).
CREATE UNIQUE INDEX crm_warehouses_one_default
    ON crm_warehouses (community_id, space_channel_id) WHERE is_default;

SELECT attach_community_write_fence('crm_spaces');
SELECT attach_community_write_fence('crm_warehouses');
