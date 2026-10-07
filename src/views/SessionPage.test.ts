import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import SessionPage from "./SessionPage.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import { premiumLocked } from "../plugins";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push, back: vi.fn() }) }));

async function type(wrapper: ReturnType<typeof mount>, digits: string) {
  for (const digit of digits) {
    await wrapper.find(`[data-test='key-${digit}']`).trigger("click");
  }
}

// Hidden sessions: a pad on screen, six dots, no keyboard, no name, no title. Six digits and it
// goes: the same gesture creates a session or enters one, and nothing says which.
describe("SessionPage", () => {
  beforeEach(() => {
    push.mockClear();
    seed();
  });

  it("shows a pad with the ten digits, a delete key and six empty dots", () => {
    const wrapper = mount(SessionPage, { shallow: true });
    for (const digit of "0123456789") {
      expect(wrapper.find(`[data-test='key-${digit}']`).text()).toBe(digit);
    }
    expect(wrapper.find("[data-test='key-delete']").exists()).toBe(true);
    expect(wrapper.findAll("[data-test='dot']")).toHaveLength(6);
    expect(wrapper.findAll("[data-test='dot'].is-filled")).toHaveLength(0);
    expect(wrapper.find("input").exists()).toBe(false);
    expect(wrapper.find("h1, h2, ion-title").exists()).toBe(false);
  });

  it("fills a dot per digit and empties one on delete", async () => {
    const wrapper = mount(SessionPage, { shallow: true });
    await type(wrapper, "246");
    expect(wrapper.findAll("[data-test='dot'].is-filled")).toHaveLength(3);
    await wrapper.find("[data-test='key-delete']").trigger("click");
    expect(wrapper.findAll("[data-test='dot'].is-filled")).toHaveLength(2);
  });

  it("opens the session on the sixth digit and lands on the chats", async () => {
    const wrapper = mount(SessionPage, { shallow: true });
    await type(wrapper, "24681");
    expect(calls.some(([command]) => command === "core_session_open")).toBe(false);
    await type(wrapper, "0");
    await flushPromises();
    expect(calls).toContainEqual(["core_session_open", { pin: "246810" }]);
    expect(push).toHaveBeenCalledWith("/tabs/chats");
  });

  // A3: every PIN is valid and takes the same path; there is no second gesture to make one.
  it("has no way to make a session other than typing its pin", () => {
    const wrapper = mount(SessionPage, { shallow: true });
    expect(wrapper.find("[data-test='session-new']").exists()).toBe(false);
  });

  // Ioan, 2026-10-08 (§108): after the free days, without the subscription, the pad is not shown at
  // all: the lock and Subscribe stand in front of it, so no PIN is typed and nothing is told.
  it("shows the lock instead of the pad once the free days are over", async () => {
    installTauri((command) => (command === "core_plan" ? { state: "limited", until: 0 } : undefined));
    const wrapper = mount(SessionPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='key-1']").exists()).toBe(false);
    expect(wrapper.find("[data-test='locked']").exists()).toBe(true);
    await wrapper.find("[data-test='subscribe']").trigger("click");
    expect(push).toHaveBeenCalledWith("/plan");
    premiumLocked.value = false;
  });

  // The free days ending with the pad open: the core refuses any PIN alike, and the user is taken to
  // the Plan screen, with nothing said about the PIN.
  it("goes to the Plan screen when the core refuses a session", async () => {
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_session_open") throw "needs_subscription";
      return command === "core_plan" ? { state: "trial", until: Date.now() + 1e9 } : undefined;
    });
    const wrapper = mount(SessionPage, { shallow: true });
    await flushPromises();
    await type(wrapper, "123456");
    await flushPromises();
    expect(push).toHaveBeenCalledWith("/plan");
    expect(push).not.toHaveBeenCalledWith("/tabs/chats");
  });
});
