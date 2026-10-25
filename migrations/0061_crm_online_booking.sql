-- CRM online booking page (kind 42135): one settings row per space.
--
-- `settings` is the normalized kind 42135 payload; `enabled` is copied out so
-- the public page can be looked up without parsing it. Bookings made on the
-- page are appointments with source 'online' (kind 42134, signed by the
-- relay) and live in crm_appointments.
CREATE TABLE crm_online_booking (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    space_channel_id   UUID        NOT NULL,
    enabled            BOOLEAN     NOT NULL,
    settings           JSONB       NOT NULL CHECK (jsonb_typeof(settings) = 'object'),
    revision_event_id  BYTEA       NOT NULL CHECK (length(revision_event_id) = 32),
    created_by         BYTEA       NOT NULL CHECK (length(created_by) = 32),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, space_channel_id),
    FOREIGN KEY (community_id, space_channel_id) REFERENCES crm_spaces (community_id, channel_id)
);

SELECT attach_community_write_fence('crm_online_booking');
