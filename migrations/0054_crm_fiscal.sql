-- CRM payments, fiscal receipts and cash-register shifts (kinds 42104, 42120,
-- 42121; docs/crm/adr/0003).
--
-- Every sale gets one `crm_sale_payments` row per payment method (one row
-- for a single-method sale), so analytics can split a mixed payment.
ALTER TABLE crm_sales
    ADD COLUMN cash_tendered_minor BIGINT
        CHECK (cash_tendered_minor IS NULL OR cash_tendered_minor BETWEEN 0 AND 100000000000);

CREATE TABLE crm_sale_payments (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    sale_id            UUID        NOT NULL,
    method             TEXT        NOT NULL CHECK (method IN ('cash', 'card', 'transfer')),
    amount_minor       BIGINT      NOT NULL CHECK (amount_minor BETWEEN 0 AND 100000000000),
    PRIMARY KEY (community_id, sale_id, method),
    FOREIGN KEY (community_id, sale_id) REFERENCES crm_sales (community_id, sale_id)
);

INSERT INTO crm_sale_payments (community_id, sale_id, method, amount_minor)
SELECT community_id, sale_id, payment, total_minor FROM crm_sales;

-- What the ККМ returned for a sale receipt or the return receipt of a voided
-- sale. One of each per sale; a fiscal document number is unique per ФН.
CREATE TABLE crm_fiscal_receipts (
    community_id         UUID        NOT NULL REFERENCES communities(id),
    sale_id              UUID        NOT NULL,
    operation            TEXT        NOT NULL CHECK (operation IN ('sell', 'sell_return')),
    space_channel_id     UUID        NOT NULL,
    total_minor          BIGINT      NOT NULL CHECK (total_minor BETWEEN 0 AND 100000000000),
    task_uuid            UUID        NOT NULL,
    document_number      BIGINT      NOT NULL CHECK (document_number > 0),
    document_sign        TEXT        NOT NULL CHECK (document_sign ~ '^[0-9]{1,32}$'),
    receipt_number       BIGINT      NOT NULL CHECK (receipt_number >= 0),
    shift_number         BIGINT      NOT NULL CHECK (shift_number > 0),
    fn_number            TEXT        NOT NULL CHECK (fn_number ~ '^[0-9]{1,32}$'),
    registration_number  TEXT        CHECK (registration_number IS NULL OR registration_number ~ '^[0-9]{1,32}$'),
    document_datetime    TEXT        NOT NULL CHECK (char_length(document_datetime) BETWEEN 1 AND 40),
    event_id             BYTEA       NOT NULL CHECK (length(event_id) = 32),
    recorded_by          BYTEA       NOT NULL CHECK (length(recorded_by) = 32),
    recorded_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, sale_id, operation),
    UNIQUE (community_id, fn_number, document_number),
    FOREIGN KEY (community_id, sale_id) REFERENCES crm_sales (community_id, sale_id)
);

-- Shifts as recorded from the ККМ, keyed by the device's own numbering.
CREATE TABLE crm_cash_shifts (
    community_id           UUID        NOT NULL REFERENCES communities(id),
    space_channel_id       UUID        NOT NULL,
    fn_number              TEXT        NOT NULL CHECK (fn_number ~ '^[0-9]{1,32}$'),
    shift_number           BIGINT      NOT NULL CHECK (shift_number > 0),
    open_document_number   BIGINT      CHECK (open_document_number IS NULL OR open_document_number > 0),
    opened_datetime        TEXT,
    opened_by              BYTEA       CHECK (opened_by IS NULL OR length(opened_by) = 32),
    close_document_number  BIGINT      CHECK (close_document_number IS NULL OR close_document_number > 0),
    closed_datetime        TEXT,
    closed_by              BYTEA       CHECK (closed_by IS NULL OR length(closed_by) = 32),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, fn_number, shift_number),
    FOREIGN KEY (community_id, space_channel_id) REFERENCES crm_spaces (community_id, channel_id)
);

CREATE INDEX crm_cash_shifts_recent
    ON crm_cash_shifts (community_id, space_channel_id, updated_at DESC);

SELECT attach_community_write_fence('crm_sale_payments');
SELECT attach_community_write_fence('crm_fiscal_receipts');
SELECT attach_community_write_fence('crm_cash_shifts');
