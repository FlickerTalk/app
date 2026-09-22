import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import BlockedPage from "./BlockedPage.vue";
import { calls, seed } from "../__tests__/seed";
import { store } from "../core";

vi.mock("vue-router", () => ({ useRouter: () => ({ back: vi.fn() }) }));

// §35: blocking is local; this is the list to undo it.
describe("BlockedPage", () => {
  beforeEach(() => seed());

  it("says when nobody is blocked", () => {
    expect(mount(BlockedPage, { shallow: true }).find("[data-test='empty']").exists()).toBe(true);
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
});
