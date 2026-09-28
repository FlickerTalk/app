import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import BackupPage from "./BackupPage.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";

vi.mock("vue-router", () => ({ useRouter: () => ({ push: vi.fn(), back: vi.fn() }) }));

type Status = { state: string; provider: string | null; drive: Record<string, unknown> | null; problem: string | null; triesLeft: number; retryAt: number | null };
const status: Status = { state: "none", provider: null, drive: null, problem: null, triesLeft: 5, retryAt: null };
const PHRASE = "a long phrase of mine";
const SUGGESTED = "ABCDE-FGHJK-MNPQR-STVWX-YZ012-34567";
const READY = { files: 3, folders: 1, used: 2_500_000, pending: 0, quota: null, backupAt: null };

/** The core, as the page sees it: the state moves as the commands are called. */
function bridge() {
  installTauri((command, args) => {
    calls.push([command, args]);
    if (command === "core_vault_status") return { ...status };
    if (command === "core_vault_connect") {
      Object.assign(status, { state: "empty", provider: "google" });
      return { ...status };
    }
    if (command === "core_vault_suggest_phrase") return SUGGESTED;
    if (command === "core_vault_setup") {
      Object.assign(status, { state: "ready", drive: { ...READY } });
      return undefined;
    }
    if (command === "core_vault_change_phrase") return undefined;
    if (command === "core_vault_unlock") {
      if (args?.phrase !== PHRASE) {
        status.triesLeft -= 1;
        throw new Error("that phrase does not open this drive");
      }
      Object.assign(status, { state: "ready", drive: { ...READY }, triesLeft: 5 });
    }
    if (command === "core_share") return undefined;
    if (command === "core_vault_backup") {
      status.drive = { ...READY, backupAt: 1_800_000_000_000 };
      return { at: 1_800_000_000_000, files: 2, dbSize: 100 };
    }
    if (command === "core_vault_restore") return { at: 1_800_000_000_000, files: 2, dbSize: 100 };
    if (command === "core_vault_disconnect") Object.assign(status, { state: "none", provider: null, drive: null });
    return undefined;
  });
}

const called = (command: string) => calls.filter(([one]) => one === command);

// Plan-drive, §61: the page only asks the core; the tokens and the key never come here.
describe("BackupPage", () => {
  beforeEach(() => {
    seed();
    Object.assign(status, { state: "none", provider: null, drive: null, problem: null, triesLeft: 5, retryAt: null });
    calls.length = 0;
    bridge();
  });

  // Plan-recuperacion (2026-09-28): the user chooses the phrase, writes it twice and keeps it
  // wherever they like; the app keeps it nowhere. It is only ever typed here, in Settings.
  it("connects the cloud and sets the drive up with a phrase written twice", async () => {
    const wrapper = mount(BackupPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("No cloud connected");
    await wrapper.find("[data-test='connect']").trigger("click");
    await flushPromises();
    expect(called("core_vault_connect")[0][1]).toEqual({ provider: "google" });
    expect(wrapper.text()).toContain("has no FlickerTalk drive yet");

    await wrapper.find("[data-test='set-up']").trigger("click");
    const confirm = () => wrapper.find("[data-test='phrase-confirm']");
    expect(wrapper.text()).toContain("not in this same Google account");
    await wrapper.find("[data-test='phrase']").setValue("too short");
    await wrapper.find("[data-test='phrase-again']").setValue("too short");
    expect(confirm().attributes("disabled")).toBe("true");
    expect(wrapper.find("[data-test='phrase-rule']").text()).toContain("12");
    await wrapper.find("[data-test='phrase']").setValue(PHRASE);
    await wrapper.find("[data-test='phrase-again']").setValue(`${PHRASE}!`);
    expect(confirm().attributes("disabled")).toBe("true");
    expect(wrapper.find("[data-test='phrase-mismatch']").exists()).toBe(true);
    await wrapper.find("[data-test='phrase-again']").setValue(PHRASE);
    // A stubbed ion-button says disabled="false" rather than dropping it.
    expect(confirm().attributes("disabled")).toBe("false");
    await confirm().trigger("click");
    await flushPromises();
    expect(called("core_vault_setup")[0][1]).toEqual({ phrase: PHRASE });
    expect(wrapper.find("[data-test='phrase']").exists()).toBe(false);
    expect(wrapper.html()).not.toContain(PHRASE);
    expect(wrapper.text()).toContain("No backup yet");
    expect(wrapper.find("[data-test='usage']").text()).toContain("2.5 MB");
  });

  it("suggests a strong phrase, which can be copied or shared to keep it", async () => {
    Object.assign(status, { state: "empty", provider: "google" });
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const wrapper = mount(BackupPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='set-up']").trigger("click");
    await wrapper.find("[data-test='suggest']").trigger("click");
    await flushPromises();
    expect((wrapper.find("[data-test='phrase']").element as HTMLInputElement).value).toBe(SUGGESTED);
    expect((wrapper.find("[data-test='phrase-again']").element as HTMLInputElement).value).toBe(SUGGESTED);
    await wrapper.find("[data-test='copy-phrase']").trigger("click");
    await flushPromises();
    expect(writeText).toHaveBeenCalledWith(SUGGESTED);
    await wrapper.find("[data-test='share-phrase']").trigger("click");
    await flushPromises();
    expect(called("core_share")[0][1]).toEqual({ text: SUGGESTED });
  });

  it("opens a drive from another phone only with its phrase, and says how many tries are left", async () => {
    Object.assign(status, { state: "locked", provider: "google" });
    const wrapper = mount(BackupPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("from another phone");
    await wrapper.find("[data-test='phrase-input']").setValue("not my phrase at all");
    await wrapper.find("[data-test='unlock']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[role='alert']").text()).toContain("does not open this drive");
    expect(wrapper.find("[data-test='tries-left']").text()).toContain("4");
    await wrapper.find("[data-test='phrase-input']").setValue(PHRASE);
    await wrapper.find("[data-test='unlock']").trigger("click");
    await flushPromises();
    expect(called("core_vault_unlock").map(([, args]) => args?.phrase)).toEqual(["not my phrase at all", PHRASE]);
    expect(wrapper.find("[role='alert']").exists()).toBe(false);
    expect(wrapper.find("[data-test='back-up']").exists()).toBe(true);
  });

  it("says until when the recovery is locked after too many wrong phrases, and does not try", async () => {
    const until = new Date(2026, 8, 29, 10, 30).getTime();
    Object.assign(status, { state: "locked", provider: "google", triesLeft: 0, retryAt: until });
    const wrapper = mount(BackupPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='retry-at']").text()).toContain(new Date(until).toLocaleString());
    expect(wrapper.find("[data-test='phrase-input']").exists()).toBe(false);
    expect(called("core_vault_unlock")).toHaveLength(0);
  });

  it("asks to set up again a drive made by the first version", async () => {
    Object.assign(status, { state: "outdated", provider: "google" });
    const wrapper = mount(BackupPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("earlier version");
    await wrapper.find("[data-test='set-up']").trigger("click");
    expect(wrapper.find("[data-test='phrase']").exists()).toBe(true);
  });

  it("changes the phrase from the phone that has the drive open", async () => {
    Object.assign(status, { state: "ready", provider: "google", drive: { ...READY } });
    const wrapper = mount(BackupPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='change-phrase']").trigger("click");
    await wrapper.find("[data-test='phrase']").setValue("another phrase, a new one");
    await wrapper.find("[data-test='phrase-again']").setValue("another phrase, a new one");
    await wrapper.find("[data-test='phrase-confirm']").trigger("click");
    await flushPromises();
    expect(called("core_vault_change_phrase")[0][1]).toEqual({ phrase: "another phrase, a new one" });
    expect(wrapper.find("[data-test='phrase']").exists()).toBe(false);
    expect(wrapper.find("[role='status']").text()).toContain("new phrase");
  });

  it("backs up, restores after asking once, and forgets the cloud after asking once", async () => {
    Object.assign(status, { state: "ready", provider: "google", drive: { ...READY } });
    const wrapper = mount(BackupPage, { shallow: true });
    await flushPromises();
    // Nothing to restore yet.
    expect(wrapper.find("[data-test='restore']").exists()).toBe(false);
    await wrapper.find("[data-test='back-up']").trigger("click");
    await flushPromises();
    expect(called("core_vault_backup")).toHaveLength(1);
    expect(wrapper.find("[data-test='last-backup']").text()).toContain("Last backup");
    expect(wrapper.find("[role='status']").text()).toContain("Backed up");

    await wrapper.find("[data-test='restore']").trigger("click");
    expect(called("core_vault_restore")).toHaveLength(0);
    expect(wrapper.find("[data-test='restore-ask']").text()).toContain("Replace everything on this phone");
    await wrapper.find("[data-test='restore-confirm']").trigger("click");
    await flushPromises();
    expect(called("core_vault_restore")).toHaveLength(1);
    expect(wrapper.text()).toContain("starts again in a moment");
  });

  it("forgets the cloud on this phone after asking once, and the files stay sealed there", async () => {
    Object.assign(status, { state: "ready", provider: "google", drive: { ...READY } });
    const wrapper = mount(BackupPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='forget']").trigger("click");
    expect(wrapper.find("[data-test='forget-ask']").text()).toContain("stay in your cloud");
    await wrapper.find("[data-test='forget-confirm']").trigger("click");
    await flushPromises();
    expect(called("core_vault_disconnect")).toHaveLength(1);
    expect(wrapper.text()).toContain("No cloud connected");
  });

  it("shows why the drive could not be opened at start", async () => {
    Object.assign(status, { state: "locked", provider: "google", problem: "the login is no longer valid" });
    const wrapper = mount(BackupPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[role='alert']").text()).toContain("the login is no longer valid");
  });
});
