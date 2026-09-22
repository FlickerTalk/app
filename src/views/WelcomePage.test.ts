import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import WelcomePage from "./WelcomePage.vue";
import { isOnboarded } from "../preferences";
import { calls, fixture, seed } from "../__tests__/seed";

const replace = vi.fn();
const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ replace, push }) }));

describe("WelcomePage", () => {
  beforeEach(() => {
    localStorage.clear();
    replace.mockClear();
    seed();
  });

  it("shows the identity created on this phone", () => {
    const wrapper = mount(WelcomePage, { shallow: true });
    expect(wrapper.text()).toContain(fixture.me.id);
    expect(wrapper.text()).toContain("No account, no phone number, no email");
  });

  // The name only travels inside the Contact Card: contacts see it when they scan.
  it("asks for a name contacts will see and keeps it", async () => {
    const wrapper = mount(WelcomePage, { shallow: true });
    await wrapper.find("[data-test='name']").setValue("Ioan");
    await wrapper.find("[data-test='start']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_set_name", { name: "Ioan" }]);
  });

  it("remembers the welcome was seen and opens the chats", async () => {
    const wrapper = mount(WelcomePage, { shallow: true });
    await wrapper.find("[data-test='start']").trigger("click");
    await flushPromises();
    expect(isOnboarded()).toBe(true);
    expect(replace).toHaveBeenCalledWith("/tabs/chats");
  });

  // M4: once the user starts, the phone can be woken when the app is closed.
  it("lets the router wake this phone once started", async () => {
    const wrapper = mount(WelcomePage, { shallow: true });
    await wrapper.find("[data-test='start']").trigger("click");
    await flushPromises();
    expect(calls.map(([command]) => command)).toContain("core_enable_push");
  });

  // §60: a new phone can take the identity of the old one instead of starting from scratch.
  it("offers bringing everything from an old phone", async () => {
    await mount(WelcomePage, { shallow: true }).find("[data-test='move-from-old']").trigger("click");
    expect(push).toHaveBeenCalledWith("/move?role=new");
  });
});
