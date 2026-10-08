import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import BlockedPage from "./BlockedPage.vue";
import { pageShape } from "../__tests__/page-shape";
import { calls, seed } from "../__tests__/seed";
import { store } from "../core";

vi.mock("vue-router", () => ({ useRouter: () => ({ back: vi.fn() }) }));

// §35: blocking is local; this is the list to undo it.
describe("BlockedPage", () => {
  beforeEach(() => seed());

  it("says when nobody is blocked", () => {
    expect(mount(BlockedPage, { shallow: true }).find("[data-test='empty']").exists()).toBe(true);
  });

  // Seen in Arabic (2026-10-02): a name reads in its own direction, not in the app's.
  it("shows the names in their own direction", () => {
    store.chats[1].blocked = true;
    expect(mount(BlockedPage, { shallow: true }).find(".ft-blocked__name").attributes("dir")).toBe("auto");
  });

  it("lists the blocked contacts and unblocks them", async () => {
    const { id, name } = store.chats[1];
    store.chats[1].blocked = true;
    const wrapper = mount(BlockedPage, { shallow: true });
    const rows = wrapper.findAll("[data-test='blocked-row']");
    expect(rows).toHaveLength(1);
    expect(rows[0].text()).toContain(name);
    await rows[0].find("[aria-label='Unblock']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_block", { contact: id, blocked: false }]);
  });

  // Ionic's own shape (2026-10-09): the page's header and content are its own children, where
  // Ionic's transitions look for them, with nothing of ours in between.
  it("is an Ionic page: a header with its back button and title, then the list", () => {
    expect(pageShape(mount(BlockedPage, { shallow: true }))).toEqual(["ion-header", "ion-content"]);
  });
});
