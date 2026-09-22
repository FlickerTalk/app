import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { actions, history, resetCalls } from "../__tests__/calls-mock";
import CallsPage from "./CallsPage.vue";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));
vi.mock("../calls", async () => (await import("../__tests__/calls-mock")).callsMock());

const entry = (id: string, outgoing: boolean, outcome: string, video = false) => ({
  id,
  contact: "ft_bob",
  name: "Bob",
  outgoing,
  video,
  startedAt: new Date(2026, 8, 22, 9, 30).getTime(),
  seconds: outcome === "answered" ? 75 : 0,
  outcome,
});

// §66: the history is what happened on this phone, never invented.
describe("CallsPage", () => {
  beforeEach(() => {
    resetCalls();
    push.mockReset();
  });

  it("shows that there are no calls yet", () => {
    const wrapper = mount(CallsPage, { shallow: true });
    expect(actions.loadHistory).toHaveBeenCalled();
    expect(wrapper.findAll("[data-test='call-row']")).toHaveLength(0);
    expect(wrapper.find("[data-test='empty']").exists()).toBe(true);
  });

  it("lists the calls, missed ones marked", () => {
    history.calls = [entry("a", false, "missed"), entry("b", true, "answered", true)] as never;
    const rows = mount(CallsPage, { shallow: true }).findAll("[data-test='call-row']");
    expect(rows).toHaveLength(2);
    expect(rows[0].classes()).toContain("is-missed");
    expect(rows[0].find("[aria-label='Missed']").exists()).toBe(true);
    expect(rows[1].find("[aria-label='Outgoing']").exists()).toBe(true);
    expect(rows[1].text()).toContain("1:15");
  });

  it("calls back the same way", async () => {
    history.calls = [entry("b", true, "answered", true)] as never;
    await mount(CallsPage, { shallow: true }).find("[aria-label='Call back']").trigger("click");
    expect(push).toHaveBeenCalledWith("/call/ft_bob?video=1");
  });
});
