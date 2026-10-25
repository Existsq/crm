import { ru } from "@/features/crm/i18n/ru";
import { isValidPhone } from "@/features/crm/lib/parties";
import type { CrmStaff, CrmStaffFields } from "@/shared/api/tauriCrmStaff";

/** Rules of `buzz_crm::staff::StaffPayload`. */
export const MAX_POSITION_CHARS = 64;
export const MAX_STAFF_SERVICES = 200;

/** The staff form as typed. */
export type StaffDraft = {
  name: string;
  position: string;
  phone: string;
  serviceIds: string[];
  /** Kept as loaded; the form does not edit it yet. */
  member: string | null;
  archived: boolean;
};

export type StaffDraftField = "name" | "position" | "phone";

/** Invalid fields in form order, for focusing the first one. */
export const STAFF_DRAFT_FIELDS: readonly StaffDraftField[] = [
  "name",
  "position",
  "phone",
];

export type StaffDraftResult =
  | { ok: true; fields: CrmStaffFields }
  | { ok: false; errors: Partial<Record<StaffDraftField, string>> };

export function emptyStaffDraft(): StaffDraft {
  return {
    name: "",
    position: "",
    phone: "",
    serviceIds: [],
    member: null,
    archived: false,
  };
}

export function staffDraftFrom(staff: CrmStaff): StaffDraft {
  return {
    name: staff.name,
    position: staff.position ?? "",
    phone: staff.phone ?? "",
    serviceIds: [...staff.serviceIds],
    member: staff.member,
    archived: staff.archived,
  };
}

export function sameStaffDraft(a: StaffDraft, b: StaffDraft): boolean {
  return (
    a.name === b.name &&
    a.position === b.position &&
    a.phone === b.phone &&
    a.member === b.member &&
    a.archived === b.archived &&
    a.serviceIds.length === b.serviceIds.length &&
    a.serviceIds.every((id) => b.serviceIds.includes(id))
  );
}

export function validateStaffDraft(draft: StaffDraft): StaffDraftResult {
  const errors: Partial<Record<StaffDraftField, string>> = {};
  const name = draft.name.trim();
  if (!name) errors.name = ru.errors.nameRequired;
  const position = draft.position.trim();
  if ([...position].length > MAX_POSITION_CHARS) {
    errors.position = ru.staff.validation.position;
  }
  const phone = draft.phone.trim();
  if (phone && !isValidPhone(phone)) {
    errors.phone = ru.parties.validation.phone;
  }
  if (Object.keys(errors).length > 0) return { ok: false, errors };
  return {
    ok: true,
    fields: {
      name,
      position: position || null,
      phone: phone || null,
      serviceIds: [...new Set(draft.serviceIds)].slice(0, MAX_STAFF_SERVICES),
      member: draft.member,
      archived: draft.archived,
    },
  };
}
