import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { effectScope, ref } from "vue";
import { calls, seed } from "./__tests__/seed";
import type { ChatMessage } from "./core";
import { setLocale } from "./i18n";
import { localInput, useSendLater } from "./sendLater";

const THEIRS: ChatMessage = { id: "m1", mine: false, text: "Are you coming tonight?", time: "09:00", sentAt: 1 };

function laterIn({ text = "", outgoing = async (words: string) => ({ text: words }) as { text: string; original?: { plugin: string; text: string } } } = {}) {
  const draft = ref(text);
  const emoji = ref(true);
  const editing = ref<ChatMessage | null>(null);
  const replying = ref<ChatMessage | null>(null);
  const hooks = { outgoing: vi.fn(outgoing), keepOriginal: vi.fn(async () => {}) };
  const chat = ref("c1");
  const scope = effectScope();
  const later = scope.run(() => useSendLater({ chatId: () => chat.value, draft, emoji, editing, replying, hooks }))!;
  return { later, chat, draft, emoji, editing, replying, hooks, stop: () => scope.stop() };
}

describe("localInput", () => {
  it("writes a local time without seconds or zone, as Ionic's datetime takes it", () => {
    expect(localInput(new Date(2026, 0, 5, 7, 3, 59).getTime())).toBe("2026-01-05T07:03");
  });
});

describe("useSendLater", () => {
  beforeEach(() => seed());
  afterEach(() => vi.useRealTimers());

  it("opens only with words to send and not during an edit, an hour ahead", () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date(2026, 9, 2, 12, 0));
    const empty = laterIn({ text: "  " });
    empty.later.openSchedule();
    expect(empty.later.scheduling.value).toBe(false);
    empty.stop();

    const editing = laterIn({ text: "fixed" });
    editing.editing.value = { ...THEIRS, mine: true };
    editing.later.openSchedule();
    expect(editing.later.scheduling.value).toBe(false);
    editing.stop();

    const { later, stop } = laterIn({ text: "good morning" });
    later.openSchedule();
    expect(later.scheduling.value).toBe(true);
    expect(later.sendAt.value).toBe("2026-10-02T13:00");
    expect(later.soonest.value).toBe("2026-10-02T12:01");
    expect(later.latest.value).toBe("2027-10-02T12:00");
    stop();
  });

  it("lets a time be chosen only once the picker is shown, and only a minute ahead or later", () => {
    const { later, stop } = laterIn({ text: "soon" });
    later.openSchedule();
    expect(later.canSchedule.value).toBe(false);
    later.schedulePresented.value = true;
    expect(later.canSchedule.value).toBe(true);
    later.pickTime(localInput(Date.now() - 60_000));
    expect(later.canSchedule.value).toBe(false);
    later.pickTime(undefined);
    expect(later.sendAt.value).toBe("");
    expect(later.canSchedule.value).toBe(false);
    stop();
  });

  it("schedules the words with the message they answer and empties the composer", async () => {
    const { later, draft, emoji, replying, stop } = laterIn({ text: " good morning " });
    replying.value = THEIRS;
    later.openSchedule();
    later.schedulePresented.value = true;
    const at = new Date(Date.now() + 2 * 3_600_000);
    at.setSeconds(0, 0);
    later.pickTime(localInput(at.getTime()));
    await later.schedule();
    expect(calls).toContainEqual(["core_schedule", { contact: "c1", text: "good morning", sendAt: at.getTime(), replyTo: "m1" }]);
    expect([draft.value, emoji.value, replying.value, later.scheduling.value]).toEqual(["", false, null, false]);
    stop();
  });

  it("schedules what the conversation's tool wrote and keeps the user's words", async () => {
    const original = { plugin: "com.flickertalk.translate", text: "hello" };
    const { later, hooks, stop } = laterIn({ text: "hello", outgoing: async () => ({ text: "bonjour", original }) });
    later.openSchedule();
    later.schedulePresented.value = true;
    await later.schedule();
    expect(calls).toContainEqual(["core_schedule", expect.objectContaining({ text: "bonjour" })]);
    expect(hooks.keepOriginal).toHaveBeenCalledWith(undefined, original);
    stop();
  });

  it("schedules for the conversation it was written in when another chat opens while the tool translates", async () => {
    let translated: (value: { text: string }) => void = () => {};
    const { later, chat, stop } = laterIn({ text: "hello", outgoing: () => new Promise<{ text: string }>((resolve) => (translated = resolve)) });
    later.openSchedule();
    later.schedulePresented.value = true;
    const scheduled = later.schedule();
    chat.value = "c2";
    translated({ text: "hello" });
    await scheduled;
    expect(calls.filter(([command]) => command === "core_schedule").map(([, args]) => args?.contact)).toEqual(["c1"]);
    stop();
  });

  it("starts the week where the app's language and the phone's region do", async () => {
    vi.spyOn(navigator, "languages", "get").mockReturnValue(["es-ES"]);
    await setLocale("es");
    try {
      const { later, stop } = laterIn();
      expect(later.weekStart.value).toBe(1);
      stop();
    } finally {
      await setLocale("en");
      vi.restoreAllMocks();
    }
  });
});
