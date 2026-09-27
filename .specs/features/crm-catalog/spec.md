# CRM Catalog Specification

## Problem Statement

Stock, sales and fiscal receipts all need products: what is sold, in which
unit, at what price and with which VAT rate and fiscal item type. The CRM space
exists (crm-foundation) but has no catalog. Staff need to create, edit, find
(by name, SKU or scanned barcode) and archive products; agents need the same
through the CLI.

## Goals

- [ ] Products are signed kind 42102 revisions projected into `crm_products` /
      `crm_product_barcodes` in the event's transaction, with CAS updates and
      unique SKU and barcode per space.
- [ ] `GET /crm/products` lists and searches products for space members, with
      bounded keyset pagination and exact barcode lookup.
- [ ] The desktop "Товары" section lists, searches, creates, edits and
      archives products; money is entered and shown in rubles with kopeck
      precision and no floating-point drift.

## Out of Scope

| Feature | Reason |
| ------- | ------ |
| Stock quantities and cost | `crm/ledger` |
| Product images, variants, price lists, bundles | Not needed for v1 retail |
| Marking codes (Честный знак) flow | v1 only flags `requires_marking`; the POS blocks such goods (VISION_CRM) |
| Import/export of the catalog (CSV) | Later |
| Categories as a managed tree | A free-text category is enough for filtering and analytics in v1 |

---

## Assumptions & Open Questions

| Assumption / decision | Chosen default | Rationale | Confirmed? |
| --------------------- | -------------- | --------- | ---------- |
| Kind | 42102 regular event, `crm-id` = product UUID, `expected-revision` on update | Same master-data model as warehouses (ADR-0001) | y |
| Who may write | Roles with `edit_catalog`: owner, admin, member, bot | ADR-0002 table | y |
| Price | `price_minor` integer kopecks, 0 ≤ price ≤ 1e11 | AD-004 | y |
| Units | `piece`, `kilogram`, `gram`, `liter`, `milliliter`, `meter`, `square_meter`, `hour`, `service_unit`; pieces are sold in whole units only | Covers retail goods and services; maps onto FFD 1.2 measure codes in `buzz-atol` | y |
| Fiscal item type | `payment_object` ∈ `commodity`, `excise`, `job`, `service`, `another` | FFD tag 1212 values used by ATOL; marked goods come later | y |
| VAT | product `vat` from `VatRate`; historical 20 % rates rejected for new revisions | 22 % from 2026 | y |
| SKU | optional, 1–64 chars `[A-Za-z0-9._/-]`, unique per space among non-null | Common retail article numbers | y |
| Barcodes | 0–20 per product; numeric 8/12/13/14-digit codes must pass the GS1 check digit; other codes 4–32 chars `[A-Za-z0-9-]`; unique per space | Scanners emit EAN/UPC/ITF; in-store codes vary | y |
| Archive | `archived: true` hides the product from default lists and the POS; archived products keep SKU/barcodes reserved | Documents reference products forever | y |
| Search | case-insensitive substring on name, prefix on SKU, exact barcode | Small catalogs; no pg_trgm dependency | y |
| Page size | default 50, max 200, keyset on (lower(name), product_id) | Bounded reads (rule 4) | y |

**Open questions:** none - all resolved or logged above.

---

## User Stories

### P1: Create and edit products ⭐ MVP

**User Story**: As a shop employee, I want to add and edit products so that they can be stocked and sold.

**Acceptance Criteria**:

1. WHEN a member with `edit_catalog` submits a valid kind 42102 without `expected-revision` for a new product id THEN the relay SHALL store the product and its barcodes in the event's transaction.
2. WHEN such a member submits a kind 42102 whose `expected-revision` equals the product's current revision THEN the relay SHALL replace the product fields and barcode set and set the revision to the new event id.
3. IF the `expected-revision` is stale THEN the relay SHALL reject it with `conflict:` (HTTP 409) and leave the product unchanged.
4. IF another product in the space already has the SKU or one of the barcodes THEN the relay SHALL reject the write with a message starting `conflict:` naming the duplicate value.
5. IF the payload fails validation (name, price, unit, VAT, payment object, SKU or barcode rules) THEN the relay SHALL reject it with a message starting `invalid:`.
6. IF a guest submits kind 42102 THEN the relay SHALL reject it with `restricted:`.

### P1: Find products ⭐ MVP

**User Story**: As a cashier, I want to find a product by name, article or scanned barcode.

**Acceptance Criteria**:

1. WHEN a space member calls `GET /crm/products` THEN the relay SHALL return non-archived products ordered by name, at most `limit` (default 50, max 200) and a `next` cursor when more exist.
2. WHEN `q` is given THEN the relay SHALL return products whose name contains `q` (case-insensitive) or whose SKU starts with `q`.
3. WHEN `barcode` is given THEN the relay SHALL return exactly the product owning that barcode, or an empty list.
4. WHEN `archived=true` is given THEN the relay SHALL include archived products.
5. IF the caller is not a member of the space channel THEN the relay SHALL answer 403.

### P1: Catalog screen ⭐ MVP

**User Story**: As a shop employee, I want a "Товары" screen so that I manage the catalog without the CLI.

**Acceptance Criteria**:

1. WHEN the "Товары" tab opens THEN the desktop SHALL show a table with name, SKU, unit, price (formatted as rubles, e.g. `1 234,50 ₽`) and VAT, and a search field.
2. WHEN the user types in the search field THEN the desktop SHALL query `GET /crm/products?q=` debounced by 250 ms.
3. WHEN a user with `edit_catalog` saves the product dialog THEN the desktop SHALL submit kind 42102 and show the product in the table.
4. IF the price field contains more than two fractional digits, a negative value or text THEN the dialog SHALL show a validation message and SHALL NOT submit.
5. WHERE the user lacks `edit_catalog` the screen SHALL be read-only.
6. IF a save fails with `conflict:` THEN the dialog SHALL stay open with the mapped Russian message.

### P2: Money handling shared with the relay

**Acceptance Criteria**:

1. The desktop money parser/formatter SHALL reproduce every case in `crates/buzz-crm/fixtures/money_vectors.json` exactly (golden test).
2. The desktop SHALL parse "1 234,50", "1234.5" and "1234" as 123450, 123450 and 123400 kopecks.

### P2: CLI

**Acceptance Criteria**:

1. WHEN `buzz crm products list [--query Q] [--barcode B]` runs THEN the CLI SHALL print the `GET /crm/products` JSON.
2. WHEN `buzz crm products create|update|archive` runs THEN the CLI SHALL publish kind 42102 and print `{event_id, accepted, message}` plus `product_id` on create.

---

## Edge Cases

- WHEN a product is archived THEN its barcodes SHALL stay reserved (a new product cannot take them) until it is restored.
- IF an update removes a barcode THEN another product SHALL be able to use it afterwards.
- WHEN a unit is `piece` THEN `min_stock_milli` SHALL be a whole number of units.
- IF two creates of one product id race THEN exactly one SHALL win (`conflict:` for the other).

---

## Requirement Traceability

| Requirement ID | Story | Phase | Status |
| -------------- | ----- | ----- | ------ |
| CRMC-01 | P1: Create and edit (AC1–AC3) | Execute | Pending |
| CRMC-02 | P1: Create and edit (AC4–AC6) | Execute | Pending |
| CRMC-03 | P1: Find products (AC1–AC5) | Execute | Pending |
| CRMC-04 | P1: Catalog screen (AC1–AC6) | Execute | Pending |
| CRMC-05 | P2: Money handling | Execute | Pending |
| CRMC-06 | P2: CLI | Execute | Pending |
