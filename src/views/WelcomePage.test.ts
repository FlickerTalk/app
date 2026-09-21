import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import WelcomePage from "./WelcomePage.vue";
import data from "../mock/chats.json";
import { isOnboarded } from "../preferences";

const replace = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ replace }) }));

describe("WelcomePage", () => {
  beforeEach(() => {
    localStorage.clear();
    replace.mockClear();
  });

  it("shows the identity created on this phone", () => {
    const wrapper = mount(WelcomePage, { shallow: true });
    expect(wrapper.text()).toContain(data.me.id);
    expect(wrapper.text()).toContain("No account, no phone number, no email");
  });

  it("remembers the welcome was seen and opens the chats", async () => {
    const wrapper = mount(WelcomePage, { shallow: true });
    await wrapper.find("[data-test='start']").trigger("click");
    expect(isOnboarded()).toBe(true);
    expect(replace).toHaveBeenCalledWith("/tabs/chats");
  });
});
