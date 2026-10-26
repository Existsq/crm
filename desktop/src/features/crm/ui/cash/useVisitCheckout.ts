import { useQuery } from "@tanstack/react-query";

import { newSaleLine, type SaleLineDraft } from "@/features/crm/lib/saleDraft";
import {
  type CrmAppointment,
  getCrmAppointment,
} from "@/shared/api/tauriCrmAppointments";
import { getCrmProduct } from "@/shared/api/tauriCrmProducts";
import { type CrmParty, getCrmParty } from "@/shared/api/tauriCrmSales";

export type VisitCheckout = {
  visit: CrmAppointment;
  /** One line per booked service at the price agreed for the visit. */
  lines: SaleLineDraft[];
  customer: CrmParty | null;
};

/**
 * The cart for paying booked visit `visitId`. Kept outside the CRM query
 * root so live refreshes do not reload a cart that is being rung up.
 */
export function useVisitCheckout(visitId: string | null) {
  return useQuery({
    queryKey: ["crm-visit-checkout", visitId],
    queryFn: async (): Promise<VisitCheckout> => {
      const visit = await getCrmAppointment(visitId ?? "");
      const [products, customer] = await Promise.all([
        Promise.all(visit.services.map((s) => getCrmProduct(s.productId))),
        visit.customerId ? getCrmParty(visit.customerId) : null,
      ]);
      const lines = visit.services.map((service, index) =>
        newSaleLine({
          id: service.productId,
          name: products[index].name,
          unit: products[index].unit,
          priceMinor: service.priceMinor,
        }),
      );
      return { visit, lines, customer };
    },
    enabled: visitId !== null,
    gcTime: 0,
    staleTime: Number.POSITIVE_INFINITY,
  });
}
