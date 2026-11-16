import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const platforms = vi.hoisted(() => ({ on: [] as string[] }));
vi.mock("@ionic/vue", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@ionic/vue")>()),
  isPlatform: (name: string) => platforms.on.includes(name),
}));

import { canInvite, inviteFriend, inviteText } from "./invite";
import { store } from "./core";
import { setLocale } from "./i18n";
import { installTauri, type Answer } from "./__tests__/tauri";

const link = "https://flickertalk.com/add#card";
const calls: Array<[string, Record<string, unknown> | undefined]> = [];

function core(answer: Answer) {
  calls.length = 0;
  installTauri((command, args) => {
    calls.push([command, args]);
    return answer(command, args);
  });
}

describe("inviting a friend", () => {
  beforeEach(() => {
    store.me.name = "Ioan";
    core(() => undefined);
  });

  afterEach(async () => {
    platforms.on = [];
    await setLocale("en");
  });

  it("is offered on Android and iOS phones and tablets, never on a desktop", () => {
    platforms.on = ["android", "mobile"];
    expect(canInvite()).toBe(true);
    platforms.on = ["ios", "ipad", "tablet"];
    expect(canInvite()).toBe(true);
    platforms.on = ["desktop"];
    expect(canInvite()).toBe(false);
    platforms.on = [];
    expect(canInvite()).toBe(false);
  });

  it("greets the friend by name and says who is writing", () => {
    expect(inviteText("Ana", "Ioan", link)).toBe(`Hi Ana, it's Ioan. I'm on FlickerTalk. Install it and tap here: ${link}`);
  });

  it("leaves out whatever name is missing", () => {
    expect(inviteText("Ana", "", link)).toBe(`Hi Ana, I'm on FlickerTalk. Install it and tap here: ${link}`);
    expect(inviteText("", "Ioan", link)).toBe(`Hi, it's Ioan. I'm on FlickerTalk. Install it and tap here: ${link}`);
    expect(inviteText(" ", "  ", link)).toBe(`Hi, I'm on FlickerTalk. Install it and tap here: ${link}`);
  });

  it("writes the invitation in the app's language", async () => {
    await setLocale("es");
    expect(inviteText("Ana", "Ioan", link)).toBe(`Hola, Ana, soy Ioan. Estoy en FlickerTalk. Instálala y toca aquí: ${link}`);
  });

  it("invites the picked friend with their WhatsApp number and my own name", async () => {
    core((command) => (command === "core_pick_contact" ? { name: "Ana", whatsapp: "34612345678" } : undefined));
    await inviteFriend(link);
    expect(calls).toEqual([
      ["core_pick_contact", {}],
      ["core_invite", { whatsapp: "34612345678", text: `Hi Ana, it's Ioan. I'm on FlickerTalk. Install it and tap here: ${link}` }],
    ]);
  });

  it("lets the core share the invitation when the friend has no WhatsApp number", async () => {
    store.me.name = "";
    core((command) => (command === "core_pick_contact" ? { name: "", whatsapp: null } : undefined));
    await inviteFriend(link);
    expect(calls[1]).toEqual(["core_invite", { whatsapp: null, text: `Hi, I'm on FlickerTalk. Install it and tap here: ${link}` }]);
  });

  it("does nothing when the user closes the picker", async () => {
    core(() => null);
    await inviteFriend(link);
    expect(calls).toEqual([["core_pick_contact", {}]]);
  });

  it("shares an invitation without a name when the phone has no contact picker", async () => {
    core((command) => {
      if (command === "core_pick_contact") throw new Error("no contact picker");
      return undefined;
    });
    await inviteFriend(link);
    expect(calls[1]).toEqual(["core_invite", { whatsapp: null, text: `Hi, it's Ioan. I'm on FlickerTalk. Install it and tap here: ${link}` }]);
  });

  it("never fails the screen when nothing could be opened", async () => {
    core((command) => {
      if (command === "core_pick_contact") return { name: "Ana", whatsapp: null };
      throw new Error("not available on this platform");
    });
    await expect(inviteFriend(link)).resolves.toBeUndefined();
  });
});
