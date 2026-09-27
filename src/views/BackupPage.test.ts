import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import BackupPage from "./BackupPage.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";

vi.mock("vue-router", () => ({ useRouter: () => ({ push: vi.fn(), back: vi.fn() }) }));

type Status = { state: string; provider: string | null; drive: Record<string, unknown> | null; problem: string | null };
const status: Status = { state: "none", provider: null, drive: null, problem: null };
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
    if (command === "core_vault_setup") {
      Object.assign(status, { state: "ready", drive: { ...READY } });
      return "ABCDE-FGHJK-MNPQR-STVWX-YZ012-34567";
    }
    if (command === "core_vault_unlock") {
      if (args?.code !== "GOOD") throw new Error("that is not the recovery code of this drive");
      Object.assign(status, { state: "ready", drive: { ...READY } });
    }
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
    Object.assign(status, { state: "none", provider: null, drive: null, problem: null });
    calls.length = 0;
    bridge();
  });

  it("connects the cloud, sets the drive up and shows the recovery code once", async () => {
    const wrapper = mount(BackupPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("No cloud connected");
    await wrapper.find("[data-test='connect']").trigger("click");
    await flushPromises();
    expect(called("core_vault_connect")[0][1]).toEqual({ provider: "google" });
    expect(wrapper.text()).toContain("has no FlickerTalk drive yet");

    await wrapper.find("[data-test='set-up']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='recovery-code']").text()).toBe("ABCDE-FGHJK-MNPQR-STVWX-YZ012-34567");
    expect(wrapper.text()).toContain("shown only once");
    await wrapper.find("[data-test='code-done']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='recovery-code']").exists()).toBe(false);
    expect(wrapper.text()).toContain("No backup yet");
    expect(wrapper.find("[data-test='usage']").text()).toContain("2.5 MB");
  });

  it("opens a drive from another phone only with its code, and says when it is wrong", async () => {
    Object.assign(status, { state: "locked", provider: "google" });
    const wrapper = mount(BackupPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("from another phone");
    await wrapper.find("[data-test='code-input']").setValue("WRONG");
    await wrapper.find("[data-test='unlock']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[role='alert']").text()).toContain("not the recovery code");
    await wrapper.find("[data-test='code-input']").setValue("GOOD");
    await wrapper.find("[data-test='unlock']").trigger("click");
    await flushPromises();
    expect(called("core_vault_unlock").map(([, args]) => args?.code)).toEqual(["WRONG", "GOOD"]);
    expect(wrapper.find("[role='alert']").exists()).toBe(false);
    expect(wrapper.find("[data-test='back-up']").exists()).toBe(true);
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
