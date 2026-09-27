-- CRM product catalog (kind 42102, docs/crm/adr/0001).
--
-- `crm_products` holds the current revision of each product; the signed
-- kind 42102 events are the history. SKU and barcodes are unique per space,
-- including archived products (documents reference products forever, so an
-- archived product keeps its codes until it is restored or edited).
CREATE TABLE crm_products (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    product_id         UUID        NOT NULL,
    space_channel_id   UUID        NOT NULL,
    name               TEXT        NOT NULL CHECK (char_length(name) BETWEEN 1 AND 128),
    sku                TEXT        CHECK (sku IS NULL OR char_length(sku) BETWEEN 1 AND 64),
    unit               TEXT        NOT NULL,
    price_minor        BIGINT      NOT NULL CHECK (price_minor BETWEEN 0 AND 100000000000),
    vat                TEXT        NOT NULL,
    payment_object     TEXT        NOT NULL,
    category           TEXT        CHECK (category IS NULL OR char_length(category) BETWEEN 1 AND 64),
    min_stock_milli    BIGINT      NOT NULL DEFAULT 0
                                   CHECK (min_stock_milli BETWEEN 0 AND 1000000000),
    requires_marking   BOOLEAN     NOT NULL DEFAULT FALSE,
    archived_at        TIMESTAMPTZ,
    revision_event_id  BYTEA       NOT NULL CHECK (length(revision_event_id) = 32),
    created_by         BYTEA       NOT NULL CHECK (length(created_by) = 32),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, product_id),
    FOREIGN KEY (community_id, space_channel_id) REFERENCES crm_spaces (community_id, channel_id)
);

-- SKUs compare case-insensitively: "MLK-1" and "mlk-1" are one article.
CREATE UNIQUE INDEX crm_products_sku_unique
    ON crm_products (community_id, space_channel_id, lower(sku)) WHERE sku IS NOT NULL;

-- Keyset order of GET /crm/products.
CREATE INDEX crm_products_by_name
    ON crm_products (community_id, space_channel_id, lower(name), product_id);

CREATE TABLE crm_product_barcodes (
    community_id       UUID        NOT NULL REFERENCES communities(id),
    space_channel_id   UUID        NOT NULL,
    barcode            TEXT        NOT NULL CHECK (char_length(barcode) BETWEEN 4 AND 32),
    product_id         UUID        NOT NULL,
    position           SMALLINT    NOT NULL CHECK (position BETWEEN 0 AND 19),
    PRIMARY KEY (community_id, space_channel_id, barcode),
    FOREIGN KEY (community_id, product_id) REFERENCES crm_products (community_id, product_id)
);

CREATE INDEX crm_product_barcodes_by_product
    ON crm_product_barcodes (community_id, product_id, position);

SELECT attach_community_write_fence('crm_products');
SELECT attach_community_write_fence('crm_product_barcodes');
