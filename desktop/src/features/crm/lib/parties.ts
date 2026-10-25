import { ru } from "@/features/crm/i18n/ru";
import type {
  CrmParty,
  CrmPartyFields,
  CrmPartyType,
} from "@/shared/api/tauriCrmSales";

/** Party types in form order. */
export const CRM_PARTY_TYPES: readonly CrmPartyType[] = [
  "customer",
  "supplier",
  "both",
];

/** Text fields of the party dialog, as typed. */
export type PartyDraft = {
  partyType: CrmPartyType;
  name: string;
  phone: string;
  email: string;
  inn: string;
  address: string;
  note: string;
  archived: boolean;
};

export type PartyDraftField = "name" | "phone" | "email" | "inn";

/** Fields in focus order, for jumping to the first invalid one. */
export const PARTY_DRAFT_FIELDS: readonly PartyDraftField[] = [
  "name",
  "phone",
  "email",
  "inn",
];

export type PartyDraftErrors = Partial<Record<PartyDraftField, string>>;

export type PartyDraftResult =
  | { ok: true; fields: CrmPartyFields }
  | { ok: false; errors: PartyDraftErrors };

export function emptyPartyDraft(): PartyDraft {
  return {
    partyType: "customer",
    name: "",
    phone: "",
    email: "",
    inn: "",
    address: "",
    note: "",
    archived: false,
  };
}

export function partyDraftFrom(party: CrmParty): PartyDraft {
  return {
    partyType: party.partyType,
    name: party.name,
    phone: party.phone ?? "",
    email: party.email ?? "",
    inn: party.inn ?? "",
    address: party.address ?? "",
    note: party.note ?? "",
    archived: party.archived,
  };
}

export function samePartyDraft(a: PartyDraft, b: PartyDraft): boolean {
  return (Object.keys(a) as (keyof PartyDraft)[]).every(
    (key) => a[key] === b[key],
  );
}

/** Same rule as `buzz_crm::party::phone`. */
export function isValidPhone(phone: string): boolean {
  const digits = phone.replace(/\D/g, "").length;
  return (
    /^[0-9+()\- ]+$/.test(phone) &&
    phone.length <= 32 &&
    digits >= 5 &&
    digits <= 15
  );
}

/** Same rule as `buzz_crm::party::email`. */
export function isValidEmail(email: string): boolean {
  if (email.length > 254 || /\s/.test(email)) return false;
  const at = email.indexOf("@");
  if (at <= 0 || email.indexOf("@", at + 1) !== -1) return false;
  const domain = email.slice(at + 1);
  return (
    domain.includes(".") && !domain.startsWith(".") && !domain.endsWith(".")
  );
}

/** Russian taxpayer number (INN) checksum, same weights as `buzz_crm::validate::inn`. */
export function isValidInn(inn: string): boolean {
  if (!/^(\d{10}|\d{12})$/.test(inn)) return false;
  const digits = [...inn].map(Number);
  const check = (weights: number[]) =>
    (weights.reduce((sum, weight, index) => sum + weight * digits[index], 0) %
      11) %
    10;
  if (digits.length === 10) {
    return check([2, 4, 10, 3, 5, 9, 4, 6, 8]) === digits[9];
  }
  return (
    check([7, 2, 4, 10, 3, 5, 9, 4, 6, 8]) === digits[10] &&
    check([3, 7, 2, 4, 10, 3, 5, 9, 4, 6, 8]) === digits[11]
  );
}

function optional(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

export function validatePartyDraft(draft: PartyDraft): PartyDraftResult {
  const errors: PartyDraftErrors = {};
  const name = draft.name.trim();
  const phone = optional(draft.phone);
  const email = optional(draft.email);
  const inn = optional(draft.inn);
  if (name === "") errors.name = ru.errors.nameRequired;
  if (phone !== null && !isValidPhone(phone)) {
    errors.phone = ru.parties.validation.phone;
  }
  if (email !== null && !isValidEmail(email)) {
    errors.email = ru.parties.validation.email;
  }
  if (inn !== null && !isValidInn(inn)) {
    errors.inn = ru.parties.validation.inn;
  }
  if (Object.keys(errors).length > 0) return { ok: false, errors };
  return {
    ok: true,
    fields: {
      partyType: draft.partyType,
      name,
      phone,
      email,
      inn,
      address: optional(draft.address),
      note: optional(draft.note),
      archived: draft.archived,
    },
  };
}
