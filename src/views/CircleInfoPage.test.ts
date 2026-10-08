import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonLabel } from "@ionic/vue";
import CircleInfoPage from "./CircleInfoPage.vue";
import { pageShape } from "../__tests__/page-shape";
import { calls, seed } from "../__tests__/seed";
import { store, type Circle } from "../core";

const replace = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ replace, push: vi.fn() }), useRoute: () => ({ params: { id: "circle1" } }) }));

function friends(overrides: Partial<Circle> = {}): Circle {
  return {
    id: "circle1",
    name: "Friends",
    hue: 120,
    members: [
      { id: "ft_me", name: "Me", admin: true, me: true },
      { id: "c1", name: "Maria López", admin: false, me: false },
    ],
    admin: true,
    adminsOnly: false,
    left: false,
    unread: 0,
    time: "",
    preview: "",
    lastMine: false,
    lastSender: "",
    status: "",
    messages: [],
    ...overrides,
  };
}

// A circle's settings (2026-09-27): who is in it and who runs it; an admin changes it, anyone
// leaves, and what is for good asks once.
describe("CircleInfoPage", () => {
  beforeEach(() => {
    replace.mockClear();
    seed();
    store.circles = [friends()];
  });

  it("lists the members, says which are admins and who is this phone", () => {
    const wrapper = mount(CircleInfoPage, { shallow: true });
    const members = wrapper.findAll("[data-test='circle-member']");
    expect(members).toHaveLength(2);
    expect(members[0].text()).toContain("You");
    expect(members[0].find("[data-test='circle-admin-badge']").exists()).toBe(true);
    expect(members[1].text()).toContain("Maria López");
    expect(members[1].find("[data-test='circle-admin-badge']").exists()).toBe(false);
  });

  // Seen in Arabic (2026-10-02): a name reads in its own direction, not in the app's.
  it("shows the circle's, the members' and the candidates' names in their own direction", async () => {
    store.circles = [friends({ name: "Amigos!" })];
    const wrapper = mount(CircleInfoPage, { shallow: true });
    expect(wrapper.find("[data-test='circle-title']").attributes("dir")).toBe("auto");
    const members = wrapper.findAll(".ft-circle__member");
    expect(members).toHaveLength(2);
    for (const member of members) expect(member.attributes("dir")).toBe("auto");
    await wrapper.find("[data-test='circle-invite']").trigger("click");
    const candidates = wrapper.find("[data-test='circle-candidates']").findAllComponents(IonLabel);
    expect(candidates.length).toBeGreaterThan(0);
    for (const candidate of candidates) expect(candidate.attributes("dir")).toBe("auto");
  });

  it("lets an admin add a contact who is not in it yet", async () => {
    const wrapper = mount(CircleInfoPage, { shallow: true });
    await wrapper.find("[data-test='circle-invite']").trigger("click");
    const candidates = wrapper.find("[data-test='circle-candidates']");
    expect(candidates.text()).not.toContain("Maria López");
    expect(candidates.text()).toContain("Alex Chen");
    await candidates.find("[data-test='invite-c2']").trigger("click");
    expect(calls).toContainEqual(["core_circle_invite", { circle: "circle1", contact: "c2" }]);
  });

  it("takes a member out only after asking once", async () => {
    const wrapper = mount(CircleInfoPage, { shallow: true });
    await wrapper.find("[data-test='circle-remove']").trigger("click");
    expect(calls.some(([command]) => command === "core_circle_remove")).toBe(false);
    await wrapper.find("[data-test='circle-remove-sure']").trigger("click");
    expect(calls).toContainEqual(["core_circle_remove", { circle: "circle1", contact: "c1" }]);
  });

  it("makes a member an admin, renames and sets who writes", async () => {
    const wrapper = mount(CircleInfoPage, { shallow: true });
    await wrapper.find("[data-test='circle-toggle-admin']").trigger("click");
    expect(calls).toContainEqual(["core_circle_set_admin", { circle: "circle1", contact: "c1", admin: true }]);
    await wrapper.find("[data-test='circle-rename']").setValue("Close friends");
    await wrapper.find("[data-test='circle-save-name']").trigger("click");
    expect(calls).toContainEqual(["core_circle_rename", { circle: "circle1", name: "Close friends" }]);
    await wrapper.findComponent({ name: "IonToggle" }).vm.$emit("ionChange", { detail: { checked: true } });
    expect(calls).toContainEqual(["core_circle_admins_only", { circle: "circle1", adminsOnly: true }]);
  });

  it("shows a plain member nothing to change, only the way out", () => {
    store.circles = [friends({ admin: false })];
    const wrapper = mount(CircleInfoPage, { shallow: true });
    expect(wrapper.find("[data-test='circle-rename']").exists()).toBe(false);
    expect(wrapper.find("[data-test='circle-invite']").exists()).toBe(false);
    expect(wrapper.find("[data-test='circle-remove']").exists()).toBe(false);
    expect(wrapper.find("[data-test='circle-leave']").exists()).toBe(true);
  });

  it("leaves after asking once, and deletes a circle one is no longer in", async () => {
    const wrapper = mount(CircleInfoPage, { shallow: true });
    await wrapper.find("[data-test='circle-leave']").trigger("click");
    expect(calls.some(([command]) => command === "core_circle_leave")).toBe(false);
    await wrapper.find("[data-test='circle-leave-sure']").trigger("click");
    expect(calls).toContainEqual(["core_circle_leave", { circle: "circle1" }]);

    store.circles = [friends({ left: true, admin: false })];
    const gone = mount(CircleInfoPage, { shallow: true });
    expect(gone.find("[data-test='circle-left']").exists()).toBe(true);
    await gone.find("[data-test='circle-forget']").trigger("click");
    await gone.find("[data-test='circle-forget-sure']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_circle_forget", { circle: "circle1" }]);
    expect(replace).toHaveBeenCalledWith("/tabs/chats");
  });

  // Ionic's own shape (2026-10-09): the page's header and content are its own children, where
  // Ionic's transitions look for them, with nothing of ours in between.
  it("is an Ionic page: a header with its back button, then the circle's settings", () => {
    expect(pageShape(mount(CircleInfoPage, { shallow: true }))).toEqual(["ion-header", "ion-content"]);
  });
});
