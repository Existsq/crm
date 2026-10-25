/**
 * CRM event kinds (relay `crates/buzz-core/src/kind.rs`, docs/nips/NIP-CRM.md).
 *
 * Kept feature-local rather than in `shared/constants/kinds.ts` because the
 * mobile app does not handle CRM kinds. `crmKinds.test.mjs` fails if these
 * drift from the relay registry.
 */
export const KIND_CRM_SPACE = 42100;
export const KIND_CRM_WAREHOUSE = 42101;
export const KIND_CRM_PRODUCT = 42102;
export const KIND_CRM_PARTY = 42103;
export const KIND_CRM_SALE = 42104;
export const KIND_CRM_SALE_VOID = 42105;
export const KIND_CRM_STOCK_DOC = 42106;
export const KIND_CRM_STOCK_DOC_VOID = 42107;
export const KIND_CRM_TECH_CARD = 42108;
export const KIND_CRM_FISCAL_RECEIPT = 42120;
export const KIND_CRM_CASH_SHIFT = 42121;
export const KIND_CRM_STAFF = 42131;
export const KIND_CRM_STAFF_SCHEDULE = 42132;
export const KIND_CRM_APPOINTMENT = 42133;

/** Every CRM kind the desktop subscribes to for live refresh. */
export const CRM_KINDS: readonly number[] = [
  KIND_CRM_SPACE,
  KIND_CRM_WAREHOUSE,
  KIND_CRM_PRODUCT,
  KIND_CRM_PARTY,
  KIND_CRM_SALE,
  KIND_CRM_SALE_VOID,
  KIND_CRM_STOCK_DOC,
  KIND_CRM_STOCK_DOC_VOID,
  KIND_CRM_TECH_CARD,
  KIND_CRM_FISCAL_RECEIPT,
  KIND_CRM_CASH_SHIFT,
  KIND_CRM_STAFF,
  KIND_CRM_STAFF_SCHEDULE,
  KIND_CRM_APPOINTMENT,
];
