import * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import { formatDuration } from "@/features/crm/lib/catalog";
import { useCrmOnlineBookingQuery } from "@/features/crm/onlineBookingHooks";
import {
  useCrmServiceChoicesQuery,
  useCrmStaffQuery,
} from "@/features/crm/staffHooks";
import type { CrmChecklistItem } from "@/features/crm/ui/onlineBooking/CrmIdChecklist";
import { CrmOnlineBookingForm } from "@/features/crm/ui/onlineBooking/CrmOnlineBookingForm";
import { CrmOnlineBookingLink } from "@/features/crm/ui/onlineBooking/CrmOnlineBookingLink";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Skeleton } from "@/shared/ui/skeleton";

/** Online booking: the public page's settings and its link. */
export function CrmOnlineBookingSection({ space }: { space: CrmMemberSpace }) {
  const t = ru.onlineBooking;
  const canManage = space.capabilities.has("manage_space");
  const query = useCrmOnlineBookingQuery();
  const staffQuery = useCrmStaffQuery(false);
  const servicesQuery = useCrmServiceChoicesQuery(true);

  const staff: CrmChecklistItem[] | null = React.useMemo(
    () =>
      staffQuery.data?.map((member) => ({
        id: member.id,
        label: member.name,
        detail: member.position,
      })) ?? (staffQuery.isPending ? null : []),
    [staffQuery.data, staffQuery.isPending],
  );
  const services: CrmChecklistItem[] | null = React.useMemo(
    () =>
      servicesQuery.data
        ?.filter((service) => service.durationMin !== null)
        .map((service) => ({
          id: service.id,
          label: service.name,
          detail:
            service.durationMin === null
              ? null
              : formatDuration(service.durationMin),
        })) ?? (servicesQuery.isPending ? null : []),
    [servicesQuery.data, servicesQuery.isPending],
  );

  const booking = query.data;
  return (
    <section
      aria-labelledby="crm-online-booking-heading"
      className="space-y-4"
      data-testid="crm-online-booking"
    >
      <SectionHeader
        description={canManage ? t.description : t.readOnlyHint}
        title={<span id="crm-online-booking-heading">{t.title}</span>}
      />
      {query.isPending ? (
        <div
          aria-busy="true"
          aria-label={t.loading}
          className="space-y-2"
          role="status"
        >
          <Skeleton className="h-24 w-full" />
          <Skeleton className="h-64 w-full" />
        </div>
      ) : query.isError || !booking ? (
        <EmptyState
          action={
            <Button
              onClick={() => void query.refetch()}
              size="sm"
              variant="outline"
            >
              {ru.common.retry}
            </Button>
          }
          testId="crm-online-booking-load-error"
          title={t.loadError}
        />
      ) : (
        <div className="grid gap-6 [@container(min-width:56rem)]:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
          <CrmOnlineBookingForm
            booking={booking}
            canManage={canManage}
            services={services}
            space={space}
            staff={staff}
          />
          <div>
            <CrmOnlineBookingLink
              enabled={booking.settings?.enabled ?? false}
              saved={booking.settings !== null}
              url={booking.publicUrl}
            />
          </div>
        </div>
      )}
    </section>
  );
}
