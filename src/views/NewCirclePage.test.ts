import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import NewCirclePage from "./NewCirclePage.vue";
import { pageShape } from "../__tests__/page-shape";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import { store } from "../core";

const replace = vi.fn();
const query = vi.hoisted(() => ({ session: undefined as string | undefined }));
vi.mock("vue-router", () => ({ useRouter: () => ({ replace, push: vi.fn() }), useRoute: () => ({ query }) }));

// A new circle (2026-09-27): a name and who is in it, from the contacts of the list it is made in.
describe("NewCirclePage", () => {
  beforeEach(() => {
    replace.mockClear();
    query.session = undefined;
    seed();
  });

  it("lists the contacts to pick from and creates nothing without a name and someone", async () => {
    const wrapper = mount(NewCirclePage, { shallow: true });
    expect(wrapper.findAll("[role='checkbox']")).toHaveLength(store.chats.length);
    const create = wrapper.find("[data-test='circle-create']");
    expect(create.attributes("disabled")).toBeDefined();
    await wrapper.find("[data-test='pick-c1']").trigger("click");
    expect(create.attributes("disabled")).toBeDefined();
    await wrapper.find("[data-test='circle-name']").setValue("Friends");
    expect(create.attributes("disabled")).toBeUndefined();
  });

  // Seen in Arabic (2026-10-02): a name reads in its own direction, not in the app's.
  it("shows the contacts' names in their own direction", () => {
    const names = mount(NewCirclePage, { shallow: true }).findAll(".ft-new__row-name");
    expect(names).toHaveLength(store.chats.length);
    for (const name of names) expect(name.attributes("dir")).toBe("auto");
  });

  it("asks the core for the circle and opens it", async () => {
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_circle_create") return "circle9";
      return command === "core_conversations" ? [] : undefined;
    });
    const wrapper = mount(NewCirclePage, { shallow: true });
    await wrapper.find("[data-test='circle-name']").setValue("Friends");
    await wrapper.find("[data-test='pick-c1']").trigger("click");
    await wrapper.find("[data-test='pick-c2']").trigger("click");
    await wrapper.find("[data-test='pick-c2']").trigger("click");
    await wrapper.find("[data-test='circle-create']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_circle_create", { name: "Friends", members: ["c1"], session: undefined }]);
    expect(replace).toHaveBeenCalledWith("/circle/circle9");
  });

  it("makes a circle of a session's contacts when opened from it", async () => {
    query.session = "s1";
    store.sessions = [{ id: "s1", chats: [{ ...store.chats[0], id: "ft_pablo", name: "Pablo" }], requests: [], circles: [] }];
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_circle_create") return "circle8";
      return command === "core_conversations" ? [] : undefined;
    });
    const wrapper = mount(NewCirclePage, { shallow: true });
    expect(wrapper.findAll("[role='checkbox']")).toHaveLength(1);
    await wrapper.find("[data-test='circle-name']").setValue("Poker");
    await wrapper.find("[data-test='pick-ft_pablo']").trigger("click");
    await wrapper.find("[data-test='circle-create']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_circle_create", { name: "Poker", members: ["ft_pablo"], session: "s1" }]);
  });

  // Ionic's own shape (2026-10-09): the page's header and content are its own children, where
  // Ionic's transitions look for them, with nothing of ours in between.
  it("is an Ionic page: a header with its back button and title, then the form", () => {
    expect(pageShape(mount(NewCirclePage, { shallow: true }))).toEqual(["ion-header", "ion-content"]);
  });
});
