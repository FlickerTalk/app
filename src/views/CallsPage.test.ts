import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { store } from "../core";
import { actions, history, resetCalls } from "../__tests__/calls-mock";
import CallsPage from "./CallsPage.vue";
import { pageShape } from "../__tests__/page-shape";

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
  // A page left mounted by another test would still follow the sessions.
  enableAutoUnmount(afterEach);

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

  // Seen in Arabic (2026-10-02): a name reads in its own direction, not in the app's.
  it("shows the names in their own direction", () => {
    history.calls = [entry("a", false, "missed")] as never;
    expect(mount(CallsPage, { shallow: true }).find(".ft-call__name").attributes("dir")).toBe("auto");
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

  // A session's calls show only while it is open (§108): opening or leaving one while the tab is
  // there loads the history again. Ionic keeps the tab mounted, so mounting alone would not.
  it("loads the history again when a session opens or is left", async () => {
    store.sessions = [];
    mount(CallsPage, { shallow: true });
    expect(actions.loadHistory).toHaveBeenCalledTimes(1);
    store.sessions = [{ id: "s1", chats: [], requests: [], circles: [] }];
    await nextTick();
    expect(actions.loadHistory).toHaveBeenCalledTimes(2);
    store.sessions = [{ id: "s1", chats: [], requests: [], circles: [] }];
    await nextTick();
    // A refresh of the same sessions (every change in the chats) is not a change.
    expect(actions.loadHistory).toHaveBeenCalledTimes(2);
    store.sessions = [];
    await nextTick();
    expect(actions.loadHistory).toHaveBeenCalledTimes(3);
  });

  it("calls back the same way", async () => {
    history.calls = [entry("b", true, "answered", true)] as never;
    await mount(CallsPage, { shallow: true }).find("[aria-label='Call back']").trigger("click");
    expect(push).toHaveBeenCalledWith("/call/ft_bob?video=1");
  });

  // Ionic's own shape (2026-10-09): the page's header and content are its own children, where
  // Ionic's transitions look for them, with nothing of ours in between.
  it("is an Ionic page: a header with the title, then the list", () => {
    expect(pageShape(mount(CallsPage, { shallow: true }))).toEqual(["ion-header", "ion-content"]);
  });
});
