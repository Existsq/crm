-- CRM sale documents (kinds 42104 and 42105, docs/crm/adr/0001).
--
-- A sale is written once by its kind 42104 event and never edited; a kind
-- 42105 event voids it (сторно) by setting the void columns. Lines snapshot
-- the product name, unit and VAT at sale time, so a later catalog edit does
-- not rewrite history. Amounts are computed by the relay (buzz_crm::sale).
CREATE TABLE crm_sales (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    sale_id            UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    -- Sequential per space, assigned under the space row lock.
    number             BIGINT      NOT NULL CHECK (number > 0),
    warehouse_id       UUID        NOT NULL,
    customer_id        UUID,
    payment            TEXT        NOT NULL CHECK (payment IN ('cash', 'card', 'transfer')),
    total_minor        BIGINT      NOT NULL CHECK (total_minor BETWEEN 0 AND 100000000000),
    discount_minor     BIGINT      NOT NULL CHECK (discount_minor BETWEEN 0 AND 100000000000),
    line_count         SMALLINT    NOT NULL CHECK (line_count BETWEEN 1 AND 200),
    note               TEXT        CHECK (note IS NULL OR char_length(note) BETWEEN 1 AND 500),
    sold_at            TIMESTAMPTZ NOT NULL,
    event_id           BYTEA       NOT NULL CHECK (length(event_id) = 32),
    created_by         BYTEA       NOT NULL CHECK (length(created_by) = 32),
    voided_at          TIMESTAMPTZ,
    void_event_id      BYTEA       CHECK (void_event_id IS NULL OR length(void_event_id) = 32),
    void_reason        TEXT        CHECK (void_reason IS NULL OR char_length(void_reason) BETWEEN 1 AND 256),
    voided_by          BYTEA       CHECK (voided_by IS NULL OR length(voided_by) = 32),
    PRIMARY KEY (community_id, sale_id),
    UNIQUE (community_id, space_channel_id, number),
    FOREIGN KEY (community_id, space_channel_id) REFERENCES crm_spaces (community_id, channel_id),
    FOREIGN KEY (community_id, warehouse_id) REFERENCES crm_warehouses (community_id, warehouse_id),
    FOREIGN KEY (community_id, customer_id) REFERENCES crm_parties (community_id, party_id),
    CHECK ((voided_at IS NULL) = (void_event_id IS NULL)
           AND (voided_at IS NULL) = (void_reason IS NULL)
           AND (voided_at IS NULL) = (voided_by IS NULL))
);

-- Sale journal and analytics windows.
CREATE INDEX crm_sales_by_time
    ON crm_sales (community_id, space_channel_id, sold_at DESC, sale_id);

-- A customer's purchase history and totals.
CREATE INDEX crm_sales_by_customer
    ON crm_sales (community_id, customer_id, sold_at DESC) WHERE customer_id IS NOT NULL;

CREATE TABLE crm_sale_lines (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    sale_id            UUID        NOT NULL,
    position           SMALLINT    NOT NULL CHECK (position BETWEEN 0 AND 199),
    product_id         UUID        NOT NULL,
    product_name       TEXT        NOT NULL CHECK (char_length(product_name) BETWEEN 1 AND 128),
    unit               TEXT        NOT NULL,
    vat                TEXT        NOT NULL,
    qty_milli          BIGINT      NOT NULL CHECK (qty_milli BETWEEN 1 AND 1000000000),
    price_minor        BIGINT      NOT NULL CHECK (price_minor BETWEEN 0 AND 100000000000),
    discount_minor     BIGINT      NOT NULL CHECK (discount_minor BETWEEN 0 AND 100000000000),
    amount_minor       BIGINT      NOT NULL CHECK (amount_minor BETWEEN 0 AND 100000000000),
    PRIMARY KEY (community_id, sale_id, position),
    UNIQUE (community_id, sale_id, product_id),
    FOREIGN KEY (community_id, sale_id) REFERENCES crm_sales (community_id, sale_id),
    FOREIGN KEY (community_id, product_id) REFERENCES crm_products (community_id, product_id)
);

-- Top-product analytics.
CREATE INDEX crm_sale_lines_by_product ON crm_sale_lines (community_id, product_id);

SELECT attach_community_write_fence('crm_sales');
SELECT attach_community_write_fence('crm_sale_lines');
