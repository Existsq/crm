-- CRM visit reminders (kinds 42150, 42151) and their delivery log.
--
-- `crm_reminder_settings` is the projection of the space's latest kind 42150
-- event. `crm_client_telegram` holds where each customer gets reminders: set
-- by staff through kind 42151 (`revision_event_id`) or by the Telegram bot
-- when the client opens their invite link (`linked_via = 'bot'`, no event).
--
-- `crm_reminder_deliveries` is relay-owned: one row per (visit, rule, visit
-- time). The unique key is the idempotency fence, so a rule fires at most
-- once for a visit at a given time, and a moved visit gets a fresh reminder.
-- Rows are planned `pending` (or `skipped` when the client has no chat), then
-- claimed with a short lease, re-checked against the live visit and sent;
-- transient failures retry with backoff until `attempts` runs out.
CREATE TABLE crm_reminder_settings (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    space_channel_id   UUID        NOT NULL,
    -- Validated `buzz_crm::ReminderSettingsPayload`.
    settings           JSONB       NOT NULL CHECK (jsonb_typeof(settings) = 'object'),
    enabled            BOOLEAN     NOT NULL,
    revision_event_id  BYTEA       NOT NULL CHECK (length(revision_event_id) = 32),
    updated_by         BYTEA       NOT NULL CHECK (length(updated_by) = 32),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, space_channel_id),
    FOREIGN KEY (community_id, space_channel_id) REFERENCES crm_spaces (community_id, channel_id)
);

CREATE INDEX crm_reminder_settings_enabled
    ON crm_reminder_settings (community_id) WHERE enabled;

CREATE TABLE crm_client_telegram (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    customer_id        UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    chat_id            BIGINT      CHECK (chat_id IS NULL OR chat_id <> 0),
    username           TEXT        CHECK (username IS NULL OR char_length(username) BETWEEN 1 AND 64),
    opted_out          BOOLEAN     NOT NULL DEFAULT false,
    linked_via         TEXT        NOT NULL CHECK (linked_via IN ('staff', 'bot')),
    revision_event_id  BYTEA       CHECK (revision_event_id IS NULL OR length(revision_event_id) = 32),
    linked_at          TIMESTAMPTZ,
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, customer_id),
    FOREIGN KEY (community_id, customer_id) REFERENCES crm_parties (community_id, party_id)
);

CREATE INDEX crm_client_telegram_by_chat
    ON crm_client_telegram (community_id, chat_id) WHERE chat_id IS NOT NULL;

CREATE TABLE crm_reminder_deliveries (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    delivery_id        UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    appointment_id     UUID        NOT NULL,
    -- `booked` or `before:<minutes>` (`buzz_crm::ReminderRule::key`).
    rule_key           TEXT        NOT NULL CHECK (char_length(rule_key) BETWEEN 1 AND 32),
    -- Visit time the message was planned for (`YYYY-MM-DD HH:MM`); empty for
    -- the booking confirmation, which is sent once per visit.
    slot_key           TEXT        NOT NULL CHECK (char_length(slot_key) <= 16),
    customer_id        UUID,
    client_name        TEXT        NOT NULL CHECK (char_length(client_name) BETWEEN 1 AND 128),
    chat_id            BIGINT,
    text               TEXT        NOT NULL CHECK (char_length(text) BETWEEN 1 AND 4096),
    status             TEXT        NOT NULL CHECK (status IN ('pending', 'sent', 'failed', 'skipped', 'cancelled')),
    -- Why a message was skipped, cancelled or failed (`no_telegram`,
    -- `opted_out`, `visit_changed`, `visit_passed`, `blocked`, `error`).
    reason             TEXT        CHECK (reason IS NULL OR char_length(reason) BETWEEN 1 AND 32),
    transport          TEXT        CHECK (transport IS NULL OR transport IN ('telegram', 'sandbox')),
    provider_message_id BIGINT,
    attempts           INTEGER     NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_error         TEXT        CHECK (last_error IS NULL OR char_length(last_error) <= 500),
    due_at             TIMESTAMPTZ NOT NULL,
    visit_at           TIMESTAMPTZ NOT NULL,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    sent_at            TIMESTAMPTZ,
    PRIMARY KEY (community_id, delivery_id),
    UNIQUE (community_id, appointment_id, rule_key, slot_key),
    FOREIGN KEY (community_id, appointment_id) REFERENCES crm_appointments (community_id, appointment_id)
);

CREATE INDEX crm_reminder_deliveries_due
    ON crm_reminder_deliveries (next_attempt_at) WHERE status = 'pending';
CREATE INDEX crm_reminder_deliveries_by_time
    ON crm_reminder_deliveries (community_id, space_channel_id, created_at DESC);

SELECT attach_community_write_fence('crm_reminder_settings');
SELECT attach_community_write_fence('crm_client_telegram');
SELECT attach_community_write_fence('crm_reminder_deliveries');
