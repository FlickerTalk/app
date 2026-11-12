import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke, convertFileSrc: (path: string) => path }));
const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

import {
  acceptContact,
  addContact,
  addSupportContact,
  block,
  contactDetails,
  declineContact,
  isSupport,
  myCardLink,
  removeContact,
  renameContact,
  renewLink,
  reportContact,
  reportLink,
  setHistory,
  setRules,
  type ContactRules,
} from "./contacts";
import { store } from "./store";

const commands = () => tauri.invoke.mock.calls.map(([command]) => command);

describe("contacts", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation((command: string) => Promise.resolve(command === "core_conversations" ? [] : undefined));
    opener.openUrl.mockReset();
  });

  it("answers a request, removes and blocks a contact through the core, and reloads the lists after each", async () => {
    const changes: [() => Promise<void>, string, Record<string, unknown>][] = [
      [() => acceptContact("ft_a"), "core_accept_contact", { contact: "ft_a" }],
      [() => declineContact("ft_b"), "core_decline_contact", { contact: "ft_b" }],
      [() => removeContact("ft_c"), "core_remove_contact", { contact: "ft_c" }],
      [() => block("ft_d", false), "core_block", { contact: "ft_d", blocked: false }],
    ];
    for (const [change, command, args] of changes) {
      tauri.invoke.mockClear();
      await change();
      expect(tauri.invoke.mock.calls[0]).toEqual([command, args]);
      expect(commands()[1]).toBe("core_conversations");
    }
  });

  it("adds the support contact and shows it, and goes on quietly when the core cannot", async () => {
    await addSupportContact();
    expect(commands().slice(0, 2)).toEqual(["core_add_support_contact", "core_conversations"]);
    tauri.invoke.mockRejectedValue(new Error("no card"));
    await expect(addSupportContact()).resolves.toBeUndefined();
  });

  it("knows the support contact only once the core has named it", () => {
    store.supportId = "";
    expect(isSupport("")).toBe(false);
    store.supportId = "ft_support";
    expect(isSupport("ft_support")).toBe(true);
    expect(isSupport("ft_bob")).toBe(false);
  });

  it("asks the core for this phone's card link and a new one, for a session or the main list", async () => {
    tauri.invoke.mockResolvedValue("https://flickertalk.com/add#card");
    expect(await myCardLink()).toBe("https://flickertalk.com/add#card");
    await myCardLink("s1");
    expect(await renewLink()).toBe("https://flickertalk.com/add#card");
    await renewLink("s1");
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_card", undefined],
      ["core_card", { session: "s1" }],
      ["core_renew_link", {}],
      ["core_renew_link", { session: "s1" }],
    ]);
  });

  it("adds a contact from a trimmed link, to a session or the main list", async () => {
    tauri.invoke.mockResolvedValue("ft_dave");
    expect(await addContact(" https://flickertalk.com/add#dave ")).toBe("ft_dave");
    await addContact("https://flickertalk.com/add#dave", "s1");
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_add_contact", { link: "https://flickertalk.com/add#dave", session: undefined }],
      ["core_add_contact", { link: "https://flickertalk.com/add#dave", session: "s1" }],
    ]);
  });

  it("renames a contact on this phone only, and sets its rules and history through the core", async () => {
    store.chats = [{ id: "ft_bob", name: "Bob", hue: 0, connected: false, unread: 0, time: "", preview: "", lastMine: false, lastKind: "text", status: "", blocked: false, messages: [] }];
    const rules: ContactRules = { muted: true, acceptsChat: true, acceptsCalls: false, receipts: false, typing: true };
    await renameContact("ft_bob", "  Robert ");
    await setRules("ft_bob", rules);
    await setHistory("ft_bob", 86_400, 30);
    expect(store.chats[0].name).toBe("Robert");
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_rename", { contact: "ft_bob", name: "Robert" }],
      ["core_set_rules", { contact: "ft_bob", rules }],
      ["core_set_history", { contact: "ft_bob", keepFor: 86_400, burnAfterRead: 30 }],
    ]);
  });

  it("asks the core for a contact's details", async () => {
    tauri.invoke.mockResolvedValue({ id: "ft_bob", name: "Bob" });
    expect(await contactDetails("ft_bob")).toEqual({ id: "ft_bob", name: "Bob" });
    expect(tauri.invoke).toHaveBeenCalledWith("core_contact", { contact: "ft_bob" });
  });

  it("writes a report as an email, quoting each piece of evidence", () => {
    const link = new URL(reportLink("ft_bob", "abuse", ["one", "two"]));
    expect(`${link.protocol}${link.pathname}`).toBe("mailto:info@flickertalk.com");
    expect(link.searchParams.get("subject")).toBe("Report ft_bob");
    expect(link.searchParams.get("body")).toBe("Reported: ft_bob\nReason: abuse\n\nEvidence:\n> one\n> two");
  });

  it("opens the report email and then blocks the contact", async () => {
    await reportContact("ft_bob", "spam", ["hi"]);
    expect(opener.openUrl).toHaveBeenCalledWith(reportLink("ft_bob", "spam", ["hi"]));
    expect(tauri.invoke.mock.calls[0]).toEqual(["core_block", { contact: "ft_bob", blocked: true }]);
  });

  it("blocks nobody when the report email cannot open", async () => {
    opener.openUrl.mockRejectedValue(new Error("no mail app"));
    await expect(reportContact("ft_bob", "spam", [])).rejects.toThrow();
    expect(tauri.invoke).not.toHaveBeenCalled();
  });
});
