-- CRM loyalty: bonus programme and per-customer bonus ledger (kinds 42170,
-- 42171, and `bonus_minor` on kind 42104 sales).
--
-- A customer's balance is the sum of their moves. Sales append a `cashback`
-- move (earned) and a `redeem` move (spent); a sale void appends the
-- negation of both, which may leave a balance below zero when the earned
-- bonuses were already spent.
CREATE TABLE crm_loyalty_settings (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    space_channel_id   UUID        NOT NULL,
    enabled            BOOLEAN     NOT NULL DEFAULT false,
    cashback_bp        INTEGER     NOT NULL DEFAULT 0 CHECK (cashback_bp BETWEEN 0 AND 10000),
    max_redeem_bp      INTEGER     NOT NULL DEFAULT 0 CHECK (max_redeem_bp BETWEEN 0 AND 10000),
    revision_event_id  BYTEA       NOT NULL CHECK (length(revision_event_id) = 32),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, space_channel_id),
    FOREIGN KEY (community_id, space_channel_id) REFERENCES crm_spaces (community_id, channel_id)
);

CREATE TABLE crm_bonus_moves (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    move_id            BIGINT      GENERATED ALWAYS AS IDENTITY,
    space_channel_id   UUID        NOT NULL,
    customer_id        UUID        NOT NULL,
    source             TEXT        NOT NULL
                                   CHECK (source IN ('cashback', 'redeem', 'manual', 'void')),
    delta_minor        BIGINT      NOT NULL
                                   CHECK (delta_minor <> 0 AND delta_minor BETWEEN -100000000000 AND 100000000000),
    sale_id            UUID,
    -- Manual adjustments only: their `crm-id`, recorded once.
    adjustment_id      UUID,
    reason             TEXT        CHECK (reason IS NULL OR char_length(reason) BETWEEN 1 AND 256),
    event_id           BYTEA       NOT NULL CHECK (length(event_id) = 32),
    created_by         BYTEA       NOT NULL CHECK (length(created_by) = 32),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, move_id),
    FOREIGN KEY (community_id, customer_id) REFERENCES crm_parties (community_id, party_id),
    CHECK ((source = 'manual') = (adjustment_id IS NOT NULL)),
    CHECK (source = 'manual' OR sale_id IS NOT NULL)
);

CREATE INDEX crm_bonus_moves_by_customer
    ON crm_bonus_moves (community_id, customer_id, move_id DESC);
CREATE INDEX crm_bonus_moves_by_sale
    ON crm_bonus_moves (community_id, sale_id) WHERE sale_id IS NOT NULL;
CREATE UNIQUE INDEX crm_bonus_moves_adjustment_unique
    ON crm_bonus_moves (community_id, adjustment_id) WHERE adjustment_id IS NOT NULL;

SELECT attach_community_write_fence('crm_loyalty_settings');
SELECT attach_community_write_fence('crm_bonus_moves');
