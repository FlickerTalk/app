import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import SessionPage from "./SessionPage.vue";
import { calls, seed } from "../__tests__/seed";

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
});
