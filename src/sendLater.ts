import { computed, ref, type Ref } from "vue";
import { scheduleText, type ChatMessage } from "./core";
import { firstDayOfWeek, i18n } from "./i18n";
import type { ChatHooks } from "./chatHooks";

// Ionic's datetime takes local time without seconds or zone
export function localInput(ms: number): string {
  const date = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function useSendLater(options: {
  chatId: () => string;
  draft: Ref<string>;
  emoji: Ref<boolean>;
  editing: Ref<ChatMessage | null>;
  replying: Ref<ChatMessage | null>;
  hooks: Pick<ChatHooks, "outgoing" | "keepOriginal">;
}) {
  const scheduling = ref(false);
  const sendAt = ref("");
  // on iOS Ionic's datetime in a sheet not yet presented left its day grid invisible
  const schedulePresented = ref(false);

  function openSchedule() {
    if (!options.draft.value.trim() || options.editing.value) return;
    sendAt.value = localInput(Date.now() + 3_600_000);
    scheduling.value = true;
  }

  const soonest = computed(() => localInput(Date.now() + 60_000));
  const weekStart = computed(() => firstDayOfWeek(i18n.global.locale.value));
  // the core takes a year ahead, Ionic's picker would stop at the end of this year
  const latest = computed(() => localInput(Date.now() + 365 * 24 * 3_600_000));

  function pickTime(value: unknown) {
    sendAt.value = typeof value === "string" ? value : "";
  }
  const canSchedule = computed(
    () => schedulePresented.value && Boolean(sendAt.value) && new Date(sendAt.value).getTime() >= Date.now() + 60_000,
  );

  async function schedule() {
    const text = options.draft.value.trim();
    const when = new Date(sendAt.value).getTime();
    if (!text || !canSchedule.value) return;
    const answering = options.replying.value?.id;
    options.draft.value = "";
    options.emoji.value = false;
    options.replying.value = null;
    scheduling.value = false;
    const out = await options.hooks.outgoing(text);
    const sent = await scheduleText(options.chatId(), out.text, when, answering).catch(() => undefined);
    if (out.original) await options.hooks.keepOriginal(sent, out.original);
  }

  return { scheduling, sendAt, schedulePresented, soonest, latest, weekStart, canSchedule, openSchedule, pickTime, schedule };
}
