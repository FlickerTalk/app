import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import WelcomePage from "./WelcomePage.vue";
import { pageShape } from "../__tests__/page-shape";
import { isOnboarded, isTourDue } from "../preferences";
import { calls, fixture, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import { checkOpenedLink, startOpenedLinks, takeOpened } from "../opened";
import { PRIVACY_URL, TERMS_URL } from "../legal";

const replace = vi.fn();
const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ replace, push }) }));
const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

const welcome = () => mount(WelcomePage, { shallow: true, global: { stubs: { "i18n-t": false } } });

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

  it("asks for a name contacts will see and keeps it", async () => {
    const wrapper = mount(WelcomePage, { shallow: true });
    await wrapper.find("[data-test='name']").setValue("Alice");
    await wrapper.find("[data-test='start']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_set_name", { name: "Alice" }]);
  });

  it("opens Add contact with a link that came before Start", async () => {
    const link = { kind: "add", link: "https://flickertalk.com/add#theirs", valid: true };
    installTauri((command) => (command === "core_opened_link" ? link : undefined));
    const app = { currentRoute: { value: { path: "/welcome" } }, push: vi.fn(), afterEach: () => () => undefined };
    const stop = startOpenedLinks(app as never);
    await checkOpenedLink();
    expect(app.push).not.toHaveBeenCalled();

    const wrapper = mount(WelcomePage, { shallow: true });
    await wrapper.find("[data-test='start']").trigger("click");
    await flushPromises();
    expect(replace).toHaveBeenCalledWith("/tabs/chats");
    expect(app.push).toHaveBeenCalledWith("/add-contact");
    expect(takeOpened("add")).toEqual(link);
    stop();
  });

  it("remembers the welcome was seen and opens the chats", async () => {
    const wrapper = mount(WelcomePage, { shallow: true });
    await wrapper.find("[data-test='start']").trigger("click");
    await flushPromises();
    expect(isOnboarded()).toBe(true);
    expect(replace).toHaveBeenCalledWith("/tabs/chats");
  });

  it("owes the quick tour to the identity made with Start, and not before", async () => {
    const wrapper = mount(WelcomePage, { shallow: true });
    expect(isTourDue()).toBe(false);
    await wrapper.find("[data-test='move-from-old']").trigger("click");
    expect(isTourDue()).toBe(false);
    let dueBeforeChats = false;
    replace.mockImplementationOnce(() => {
      dueBeforeChats = isTourDue();
    });
    await wrapper.find("[data-test='start']").trigger("click");
    await flushPromises();
    expect(dueBeforeChats).toBe(true);
  });

  it("lets the router wake this phone once started", async () => {
    const wrapper = mount(WelcomePage, { shallow: true });
    await wrapper.find("[data-test='start']").trigger("click");
    await flushPromises();
    expect(calls.map(([command]) => command)).toContain("core_enable_push");
  });

  it("adds the FlickerTalk contact on Start, before the chats open", async () => {
    let addedBeforeChats = false;
    replace.mockImplementationOnce(() => {
      addedBeforeChats = calls.some(([command]) => command === "core_add_support_contact");
    });
    const wrapper = mount(WelcomePage, { shallow: true });
    await wrapper.find("[data-test='start']").trigger("click");
    await flushPromises();
    expect(addedBeforeChats).toBe(true);
  });

  it("adds nothing until Start, nor for a phone that brings everything from an old one", async () => {
    const wrapper = mount(WelcomePage, { shallow: true });
    await wrapper.find("[data-test='move-from-old']").trigger("click");
    await flushPromises();
    expect(calls.map(([command]) => command)).not.toContain("core_add_support_contact");
  });

  it("offers bringing everything from an old phone", async () => {
    await mount(WelcomePage, { shallow: true }).find("[data-test='move-from-old']").trigger("click");
    expect(push).toHaveBeenCalledWith("/move?role=new");
  });

  it("says under Start that starting accepts the Terms of Use and the Privacy Policy", () => {
    const wrapper = welcome();
    const terms = wrapper.find("[data-test='terms']");
    expect(terms.text()).toBe("By tapping Start you accept the Terms of Use and the Privacy Policy.");
    const html = wrapper.html();
    expect(html.indexOf('data-test="terms"')).toBeGreaterThan(html.indexOf('data-test="start"'));
    expect(html.indexOf('data-test="terms"')).toBeLessThan(html.indexOf('data-test="move-from-old"'));
    const links = terms.findAll("a");
    expect(links.map((link) => [link.text(), link.attributes("href")])).toEqual([
      ["Terms of Use", TERMS_URL],
      ["Privacy Policy", PRIVACY_URL],
    ]);
  });

  it("opens the terms and the privacy policy in the phone's browser, without starting", async () => {
    opener.openUrl.mockReset();
    const wrapper = welcome();
    await wrapper.find("[data-test='terms-link']").trigger("click");
    await wrapper.find("[data-test='privacy-link']").trigger("click");
    expect(opener.openUrl.mock.calls).toEqual([[TERMS_URL], [PRIVACY_URL]]);
    await flushPromises();
    expect(replace).not.toHaveBeenCalled();
    expect(isOnboarded()).toBe(false);
  });

  it("asks for nothing more than Start", () => {
    const wrapper = welcome();
    expect(wrapper.find("input[type='checkbox']").exists()).toBe(false);
    expect(wrapper.find("ion-checkbox-stub").exists()).toBe(false);
  });

  it("is an Ionic page: the welcome fills its content, with no header", () => {
    expect(pageShape(mount(WelcomePage, { shallow: true }))).toEqual(["ion-content"]);
  });
});
