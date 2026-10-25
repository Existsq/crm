import * as React from "react";
import { toast } from "sonner";

import { ru } from "@/features/crm/i18n/ru";
import { parseChatId } from "@/features/crm/lib/reminders";
import {
  useCrmClientTelegramQuery,
  useCrmRemindersQuery,
  useWriteCrmClientTelegramMutation,
} from "@/features/crm/reminderHooks";
import { CrmField, crmControlProps } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmClientTelegram } from "@/shared/api/tauriCrmReminders";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Checkbox } from "@/shared/ui/checkbox";
import { Input } from "@/shared/ui/input";

const t = ru.reminders.telegram;

function statusLabel(link: CrmClientTelegram): string {
  if (!link.linked) return t.notLinked;
  if (link.username) return t.linkedAs.replace("{value}", `@${link.username}`);
  if (link.linkedVia === "staff" && link.chatId !== null) {
    return t.linkedByStaff.replace("{value}", String(link.chatId));
  }
  return t.linked;
}

/**
 * Where a customer gets visit reminders: the bot invite link to send them,
 * and in the sandbox a hand-linked test chat (kind 42151).
 */
export function CrmPartyTelegram({
  canEdit,
  channelId,
  partyId,
}: {
  canEdit: boolean;
  channelId: string;
  partyId: string;
}) {
  const query = useCrmClientTelegramQuery(partyId);
  const reminders = useCrmRemindersQuery();
  const write = useWriteCrmClientTelegramMutation();
  const [chatInput, setChatInput] = React.useState("");
  const [chatError, setChatError] = React.useState<string | undefined>();
  const sandbox = reminders.data?.transport.mode !== "telegram";
  const link = query.data;

  async function save(chatId: number | null, optedOut: boolean) {
    try {
      await write.mutateAsync({
        channelId,
        customerId: partyId,
        chatId,
        optedOut,
      });
      toast.success(t.saved);
      setChatInput("");
    } catch {
      // Rendered through the mutation error state.
    }
  }

  function linkTestChat() {
    const chatId = parseChatId(chatInput);
    if (chatId === null) {
      setChatError(t.chatIdInvalid);
      return;
    }
    setChatError(undefined);
    void save(chatId, link?.optedOut ?? false);
  }

  async function copyInvite(url: string) {
    try {
      await navigator.clipboard.writeText(url);
      toast.success(t.inviteCopied);
    } catch {
      toast.error(ru.errors.unknown);
    }
  }

  return (
    <section
      aria-labelledby="crm-party-telegram-title"
      className="space-y-3 rounded-xl border border-border/70 p-3"
      data-testid="crm-party-telegram"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-medium" id="crm-party-telegram-title">
          {t.title}
        </h3>
        {link ? (
          <Badge
            data-testid="crm-party-telegram-status"
            variant={link.linked && !link.optedOut ? "success" : "secondary"}
          >
            {statusLabel(link)}
          </Badge>
        ) : null}
      </div>
      {query.isPending ? (
        <p className="text-sm text-muted-foreground" role="status">
          {t.loading}
        </p>
      ) : query.isError ? (
        <div className="space-y-1">
          <p className="text-sm text-destructive">{t.loadError}</p>
          <CrmErrorAlert error={query.error} />
        </div>
      ) : (
        <>
          {link?.optedOut ? (
            <p className="text-sm text-muted-foreground">{t.optedOut}</p>
          ) : null}
          {link?.inviteUrl ? (
            <div className="space-y-1.5">
              <p className="text-xs text-muted-foreground">{t.inviteHint}</p>
              <div className="flex gap-2">
                <Input
                  aria-label={t.copyInvite}
                  className="font-mono text-xs"
                  data-testid="crm-party-telegram-invite"
                  readOnly
                  value={link.inviteUrl}
                />
                <Button
                  onClick={() => void copyInvite(link.inviteUrl ?? "")}
                  type="button"
                  variant="outline"
                >
                  {t.copyInvite}
                </Button>
              </div>
            </div>
          ) : (
            <p className="text-xs text-muted-foreground">{t.sandboxHint}</p>
          )}
          {canEdit && link ? (
            <div className="space-y-3">
              {sandbox ? (
                <div className="flex flex-wrap items-end gap-2">
                  <CrmField
                    className="min-w-48 flex-1"
                    error={chatError}
                    hint={t.chatIdHint}
                    id="crm-party-telegram-chat"
                    label={t.chatIdLabel}
                  >
                    <Input
                      {...crmControlProps(
                        "crm-party-telegram-chat",
                        true,
                        chatError,
                      )}
                      inputMode="numeric"
                      onChange={(event) => setChatInput(event.target.value)}
                      onKeyDown={(event) => {
                        // The block sits inside the party form: Enter links
                        // the chat instead of saving the card.
                        if (event.key === "Enter") {
                          event.preventDefault();
                          linkTestChat();
                        }
                      }}
                      value={chatInput}
                    />
                  </CrmField>
                  <Button
                    data-testid="crm-party-telegram-link"
                    disabled={write.isPending}
                    onClick={linkTestChat}
                    type="button"
                    variant="outline"
                  >
                    {t.link}
                  </Button>
                </div>
              ) : null}
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div className="flex items-start gap-2">
                  <Checkbox
                    aria-describedby="crm-party-telegram-optout-hint"
                    checked={link.optedOut}
                    data-testid="crm-party-telegram-optout"
                    disabled={write.isPending}
                    id="crm-party-telegram-optout"
                    onCheckedChange={(checked) =>
                      void save(link.chatId, checked === true)
                    }
                  />
                  <div className="space-y-0.5">
                    <label
                      className="text-sm"
                      htmlFor="crm-party-telegram-optout"
                    >
                      {t.optOut}
                    </label>
                    <p
                      className="text-xs text-muted-foreground"
                      id="crm-party-telegram-optout-hint"
                    >
                      {t.optOutHint}
                    </p>
                  </div>
                </div>
                {link.linked ? (
                  <Button
                    data-testid="crm-party-telegram-unlink"
                    disabled={write.isPending}
                    onClick={() => void save(null, link.optedOut)}
                    type="button"
                    variant="ghost"
                  >
                    {t.unlink}
                  </Button>
                ) : null}
              </div>
              <CrmErrorAlert
                error={write.error}
                testId="crm-party-telegram-error"
              />
            </div>
          ) : null}
        </>
      )}
    </section>
  );
}
