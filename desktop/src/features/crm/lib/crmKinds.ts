/**
 * CRM event kinds (relay `crates/buzz-core/src/kind.rs`, docs/nips/NIP-CRM.md).
 *
 * Kept feature-local rather than in `shared/constants/kinds.ts` because the
 * mobile app does not handle CRM kinds. `crmKinds.test.mjs` fails if these
 * drift from the relay registry.
 */
export const KIND_CRM_SPACE = 42100;
export const KIND_CRM_WAREHOUSE = 42101;

/** Every CRM kind the desktop subscribes to for live refresh. */
export const CRM_KINDS: readonly number[] = [
  KIND_CRM_SPACE,
  KIND_CRM_WAREHOUSE,
];
