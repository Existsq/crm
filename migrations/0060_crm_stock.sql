-- CRM stock ledger and service tech cards (kinds 42104–42108).
--
-- `crm_stock` is the current quantity and cost value of each product on each
-- warehouse; `crm_stock_moves` is the append-only journal whose deltas sum to
-- it. Moves come from stock documents (42106), sales (42104, goods lines and
-- the consumables of service lines) and their voids (42107, 42105), which
-- append reversal moves. All amounts are computed by the relay
-- (buzz_crm::stock) under the space row lock.
CREATE TABLE crm_stock (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    warehouse_id       UUID        NOT NULL,
    product_id         UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    qty_milli          BIGINT      NOT NULL DEFAULT 0
                                   CHECK (qty_milli BETWEEN -1000000000 AND 1000000000),
    -- Cost value of the quantity on hand; zero when nothing is on hand.
    value_minor        BIGINT      NOT NULL DEFAULT 0
                                   CHECK (value_minor BETWEEN 0 AND 100000000000),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, warehouse_id, product_id),
    FOREIGN KEY (community_id, warehouse_id) REFERENCES crm_warehouses (community_id, warehouse_id),
    FOREIGN KEY (community_id, product_id) REFERENCES crm_products (community_id, product_id),
    CHECK (qty_milli > 0 OR value_minor = 0)
);

CREATE INDEX crm_stock_by_product ON crm_stock (community_id, product_id);

-- Receipts, write-offs, inventories and transfers; posted once, voided by
-- kind 42107 (сторно).
CREATE TABLE crm_stock_docs (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    doc_id             UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    -- Sequential per space, assigned under the space row lock.
    number             BIGINT      NOT NULL CHECK (number > 0),
    doc_type           TEXT        NOT NULL
                                   CHECK (doc_type IN ('receipt', 'write_off', 'inventory', 'transfer')),
    warehouse_id       UUID        NOT NULL,
    to_warehouse_id    UUID,
    supplier_id        UUID,
    reference          TEXT        CHECK (reference IS NULL OR char_length(reference) BETWEEN 1 AND 64),
    note               TEXT        CHECK (note IS NULL OR char_length(note) BETWEEN 1 AND 500),
    line_count         SMALLINT    NOT NULL CHECK (line_count BETWEEN 1 AND 500),
    -- Receipt: purchase cost; other types: cost value moved (absolute).
    total_minor        BIGINT      NOT NULL CHECK (total_minor BETWEEN 0 AND 100000000000),
    posted_at          TIMESTAMPTZ NOT NULL,
    event_id           BYTEA       NOT NULL CHECK (length(event_id) = 32),
    created_by         BYTEA       NOT NULL CHECK (length(created_by) = 32),
    voided_at          TIMESTAMPTZ,
    void_event_id      BYTEA       CHECK (void_event_id IS NULL OR length(void_event_id) = 32),
    void_reason        TEXT        CHECK (void_reason IS NULL OR char_length(void_reason) BETWEEN 1 AND 256),
    voided_by          BYTEA       CHECK (voided_by IS NULL OR length(voided_by) = 32),
    PRIMARY KEY (community_id, doc_id),
    UNIQUE (community_id, space_channel_id, number),
    FOREIGN KEY (community_id, space_channel_id) REFERENCES crm_spaces (community_id, channel_id),
    FOREIGN KEY (community_id, warehouse_id) REFERENCES crm_warehouses (community_id, warehouse_id),
    FOREIGN KEY (community_id, to_warehouse_id) REFERENCES crm_warehouses (community_id, warehouse_id),
    FOREIGN KEY (community_id, supplier_id) REFERENCES crm_parties (community_id, party_id),
    CHECK ((doc_type = 'transfer') = (to_warehouse_id IS NOT NULL)),
    CHECK (doc_type = 'receipt' OR supplier_id IS NULL),
    CHECK ((voided_at IS NULL) = (void_event_id IS NULL)
           AND (voided_at IS NULL) = (void_reason IS NULL)
           AND (voided_at IS NULL) = (voided_by IS NULL))
);

CREATE INDEX crm_stock_docs_by_time
    ON crm_stock_docs (community_id, space_channel_id, posted_at DESC, doc_id);

CREATE TABLE crm_stock_doc_lines (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    doc_id             UUID        NOT NULL,
    position           SMALLINT    NOT NULL CHECK (position BETWEEN 0 AND 499),
    product_id         UUID        NOT NULL,
    product_name       TEXT        NOT NULL CHECK (char_length(product_name) BETWEEN 1 AND 128),
    unit               TEXT        NOT NULL,
    -- Quantity moved; for an inventory, the counted quantity.
    qty_milli          BIGINT      NOT NULL CHECK (qty_milli BETWEEN 0 AND 1000000000),
    -- Inventory only: the book quantity at posting time.
    book_qty_milli     BIGINT      CHECK (book_qty_milli BETWEEN -1000000000 AND 1000000000),
    -- Purchase cost per unit (receipts, inventory surpluses).
    cost_minor         BIGINT      CHECK (cost_minor BETWEEN 0 AND 100000000000),
    -- Cost value moved by the line (absolute).
    amount_minor       BIGINT      NOT NULL CHECK (amount_minor BETWEEN 0 AND 100000000000),
    PRIMARY KEY (community_id, doc_id, position),
    UNIQUE (community_id, doc_id, product_id),
    FOREIGN KEY (community_id, doc_id) REFERENCES crm_stock_docs (community_id, doc_id),
    FOREIGN KEY (community_id, product_id) REFERENCES crm_products (community_id, product_id)
);

CREATE TABLE crm_stock_moves (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    move_id            BIGINT      GENERATED ALWAYS AS IDENTITY,
    space_channel_id   UUID        NOT NULL,
    warehouse_id       UUID        NOT NULL,
    product_id         UUID        NOT NULL,
    source             TEXT        NOT NULL
                                   CHECK (source IN ('receipt', 'write_off', 'inventory',
                                                     'transfer', 'sale', 'consumable')),
    -- The stock document or sale that caused the move.
    doc_id             UUID        NOT NULL,
    -- A void appends the negation of the document's moves.
    reversal           BOOLEAN     NOT NULL DEFAULT FALSE,
    qty_delta_milli    BIGINT      NOT NULL,
    value_delta_minor  BIGINT      NOT NULL,
    qty_after_milli    BIGINT      NOT NULL,
    moved_at           TIMESTAMPTZ NOT NULL,
    event_id           BYTEA       NOT NULL CHECK (length(event_id) = 32),
    PRIMARY KEY (community_id, move_id),
    FOREIGN KEY (community_id, warehouse_id, product_id)
        REFERENCES crm_stock (community_id, warehouse_id, product_id)
);

-- Product card history and the movements of one document.
CREATE INDEX crm_stock_moves_by_product
    ON crm_stock_moves (community_id, product_id, move_id DESC);
CREATE INDEX crm_stock_moves_by_doc ON crm_stock_moves (community_id, doc_id);
CREATE INDEX crm_stock_moves_by_space
    ON crm_stock_moves (community_id, space_channel_id, move_id DESC);

-- Consumables per service: `crm_tech_cards` holds the current revision,
-- `crm_tech_card_items` its consumables per service unit.
CREATE TABLE crm_tech_cards (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    service_id         UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    revision_event_id  BYTEA       NOT NULL CHECK (length(revision_event_id) = 32),
    created_by         BYTEA       NOT NULL CHECK (length(created_by) = 32),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, service_id),
    FOREIGN KEY (community_id, service_id) REFERENCES crm_products (community_id, product_id),
    FOREIGN KEY (community_id, space_channel_id) REFERENCES crm_spaces (community_id, channel_id)
);

CREATE TABLE crm_tech_card_items (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    service_id         UUID        NOT NULL,
    position           SMALLINT    NOT NULL CHECK (position BETWEEN 0 AND 49),
    product_id         UUID        NOT NULL,
    qty_milli          BIGINT      NOT NULL CHECK (qty_milli BETWEEN 1 AND 1000000000),
    PRIMARY KEY (community_id, service_id, position),
    UNIQUE (community_id, service_id, product_id),
    FOREIGN KEY (community_id, service_id) REFERENCES crm_tech_cards (community_id, service_id),
    FOREIGN KEY (community_id, product_id) REFERENCES crm_products (community_id, product_id)
);

SELECT attach_community_write_fence('crm_stock');
SELECT attach_community_write_fence('crm_stock_docs');
SELECT attach_community_write_fence('crm_stock_doc_lines');
SELECT attach_community_write_fence('crm_stock_moves');
SELECT attach_community_write_fence('crm_tech_cards');
SELECT attach_community_write_fence('crm_tech_card_items');
