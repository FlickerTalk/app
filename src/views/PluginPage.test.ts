import { describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h, reactive } from "vue";
import { setLocale } from "../i18n";

const nav = vi.hoisted(() => ({ push: vi.fn(), back: vi.fn(), replace: vi.fn() }));
const route = vi.hoisted(() => ({ value: null as unknown as { params: Record<string, string>; query: Record<string, string> } }));
vi.mock("vue-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("vue-router")>()),
  useRoute: () => route.value,
  useRouter: () => nav,
}));
vi.mock("../plugins", async (importOriginal) => {
  const plugins = await importOriginal<typeof import("../plugins")>();
  plugins.installed.value = [{ id: "com.flickertalk.notes", name: "Notes", locales: { es: { name: "Notas" } } } as never];
  return { ...plugins, refreshPlugins: vi.fn(), refreshPremiumLock: vi.fn() };
});

import PluginPage from "./PluginPage.vue";
import { pageShape } from "../__tests__/page-shape";
import { installed, premiumLocked } from "../plugins";
import PluginSheet from "../components/PluginSheet.vue";
import PermissionAsk from "../components/PermissionAsk.vue";
import ContactPicker from "../components/ContactPicker.vue";
import { takeSend, type ContactAsk } from "../pending-send";

describe("PluginPage", () => {
  it("follows the reminder in the address, not only the first one", async () => {
    route.value = reactive({ params: { id: "com.flickertalk.notes" }, query: { reminder: "r1" } });
    const wrapper = mount(PluginPage, { shallow: true });
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).props("reminder")).toBe("r1");
    expect(wrapper.findComponent(PluginSheet).props("fill")).toBe(true);

    route.value.query = { reminder: "r2" };
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).props("reminder")).toBe("r2");
  });

  it("opens the plugin in the session the address names, or in none", async () => {
    route.value = reactive({ params: { id: "com.flickertalk.notes" }, query: { reminder: "r1", session: "s1" } });
    const wrapper = mount(PluginPage, { shallow: true });
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).props("session")).toBe("s1");

    route.value.query = {};
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).props("session")).toBeUndefined();
  });

  it("names the plugin in the phone's language", async () => {
    await setLocale("es");
    try {
      route.value = reactive({ params: { id: "com.flickertalk.notes" }, query: {} });
      const wrapper = mount(PluginPage, { shallow: true });
      await flushPromises();
      expect(wrapper.find("ion-title-stub").text()).toBe("Notas");
    } finally {
      await setLocale("en");
    }
  });

  describe("closing", () => {
    const close = vi.fn(async () => {});
    let mounted = 0;
    const Sheet = defineComponent({
      name: "PluginSheet",
      props: ["plugin", "contact", "reminder", "session"],
      emits: ["done", "closed", "openChat"],
      setup(_, { expose }) {
        mounted += 1;
        expose({ close });
        return () => h("div");
      },
    });
    const hooks = (wrapper: { vm: unknown }, name: string) =>
      ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>)[name] ?? []).forEach((hook) => hook());

    async function page() {
      close.mockClear();
      mounted = 0;
      route.value = reactive({ params: { id: "com.flickertalk.notes" }, query: {} });
      const wrapper = mount(PluginPage, { shallow: true, global: { stubs: { PluginSheet: Sheet } } });
      await flushPromises();
      hooks(wrapper, "onIonViewWillEnter");
      return wrapper;
    }

    it("closes the plugin through its sheet when the page is left", async () => {
      const wrapper = await page();
      expect(close).not.toHaveBeenCalled();
      hooks(wrapper, "onIonViewWillLeave");
      expect(close).toHaveBeenCalledTimes(1);
    });

    it("opens the plugin afresh when the page is back after it said goodbye", async () => {
      const wrapper = await page();
      expect(mounted).toBe(1);
      hooks(wrapper, "onIonViewWillLeave");
      wrapper.findComponent(Sheet).vm.$emit("closed");
      await flushPromises();
      expect(mounted).toBe(1);
      hooks(wrapper, "onIonViewWillEnter");
      await flushPromises();
      expect(mounted).toBe(2);
    });

    it("opens the plugin afresh at once when it said goodbye with the page on screen", async () => {
      const wrapper = await page();
      hooks(wrapper, "onIonViewWillLeave");
      hooks(wrapper, "onIonViewWillEnter");
      wrapper.findComponent(Sheet).vm.$emit("closed");
      await flushPromises();
      expect(mounted).toBe(2);
    });
  });

  it("shows a locked tool's lock and leads to the Premium section of Settings", async () => {
    premiumLocked.value = true;
    try {
      route.value = reactive({ params: { id: "com.flickertalk.notes" }, query: {} });
      const wrapper = mount(PluginPage, { shallow: true });
      await flushPromises();
      expect(wrapper.findComponent(PluginSheet).exists()).toBe(false);
      expect(wrapper.find("[data-test='locked']").exists()).toBe(true);
      await wrapper.find("[data-test='subscribe']").trigger("click");
      expect(nav.push).toHaveBeenCalledWith("/tabs/settings#premium");

      installed.value = [...installed.value, { id: "com.flickertalk.chess", name: "Chess", kind: "game" } as never];
      route.value = reactive({ params: { id: "com.flickertalk.chess" }, query: {} });
      const game = mount(PluginPage, { shallow: true });
      await flushPromises();
      expect(game.findComponent(PluginSheet).exists()).toBe(true);
    } finally {
      premiumLocked.value = false;
    }
  });

  describe("asking for a permission on the spot", () => {
    const need = () => ({ key: "location", label: "Your location, only when you ask", icon: "pin", on: false, answer: vi.fn() });
    const Sheet = defineComponent({
      name: "PluginSheet",
      props: ["plugin", "contact", "reminder", "session"],
      emits: ["done", "closed", "openChat", "needs"],
      setup(_, { expose }) {
        expose({ close: async () => {} });
        return () => h("div");
      },
    });
    async function page() {
      route.value = reactive({ params: { id: "com.flickertalk.notes" }, query: {} });
      const wrapper = mount(PluginPage, { shallow: true, global: { stubs: { PluginSheet: Sheet } } });
      await flushPromises();
      return wrapper;
    }

    it("shows the tool and the permission, and hands a yes back", async () => {
      const wrapper = await page();
      const ask = () => wrapper.findComponent(PermissionAsk);
      expect(ask().props("open")).toBe(false);
      const asked = need();
      wrapper.findComponent(Sheet).vm.$emit("needs", asked);
      await flushPromises();
      expect(ask().props("open")).toBe(true);
      expect(ask().props("name")).toBe("Notes");
      expect(ask().props("permission")).toMatchObject({ label: "Your location, only when you ask" });
      ask().vm.$emit("allow");
      await flushPromises();
      expect(asked.answer).toHaveBeenCalledWith(true);
      expect(ask().props("open")).toBe(false);
    });

    it("hands a no back on Cancel, and when the page is left", async () => {
      const wrapper = await page();
      const ask = () => wrapper.findComponent(PermissionAsk);
      const first = need();
      wrapper.findComponent(Sheet).vm.$emit("needs", first);
      await flushPromises();
      ask().vm.$emit("cancel");
      expect(first.answer).toHaveBeenCalledWith(false);

      const second = need();
      wrapper.findComponent(Sheet).vm.$emit("needs", second);
      await flushPromises();
      ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>).onIonViewWillLeave ?? []).forEach((hook) => hook());
      await flushPromises();
      expect(second.answer).toHaveBeenCalledWith(false);
      expect(ask().props("open")).toBe(false);
    });
  });

  describe("sending from outside a chat", () => {
    const Sheet = defineComponent({
      name: "PluginSheet",
      props: ["plugin", "contact", "reminder", "session", "sending"],
      emits: ["done", "closed", "openChat", "needs", "pick", "sendTo"],
      setup(_, { expose }) {
        expose({ close: async () => {} });
        return () => h("div");
      },
    });
    const ask = (): ContactAsk => ({ answer: vi.fn() });
    const hooks = (wrapper: { vm: unknown }, name: string) =>
      ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>)[name] ?? []).forEach((hook) => hook());
    async function page() {
      nav.push.mockClear();
      nav.replace.mockClear();
      nav.back.mockClear();
      route.value = reactive({ params: { id: "com.flickertalk.notes" }, query: {} });
      const wrapper = mount(PluginPage, { shallow: true, global: { stubs: { PluginSheet: Sheet } } });
      await flushPromises();
      hooks(wrapper, "onIonViewWillEnter");
      return wrapper;
    }
    const picker = (wrapper: Awaited<ReturnType<typeof page>>) => wrapper.findComponent(ContactPicker);

    it("hands the sheet what the plugin was granted for the chat", async () => {
      installed.value = [{ id: "com.flickertalk.notes", name: "Notes", granted: { send: "propose" } } as never];
      try {
        const wrapper = await page();
        expect(wrapper.findComponent(Sheet).props("sending")).toBe("propose");
      } finally {
        installed.value = [{ id: "com.flickertalk.notes", name: "Notes", locales: { es: { name: "Notas" } } } as never];
      }
      const wrapper = await page();
      expect(wrapper.findComponent(Sheet).props("sending")).toBe("nothing");
    });

    it("asks who for with the contact picker, and opens that chat with the proposal waiting", async () => {
      const wrapper = await page();
      expect(picker(wrapper).props("open")).toBe(false);
      expect(picker(wrapper).props("purpose")).toBe("send");
      const asked = ask();
      wrapper.findComponent(Sheet).vm.$emit("pick", asked);
      await flushPromises();
      expect(picker(wrapper).props("open")).toBe(true);
      picker(wrapper).vm.$emit("pick", "c2");
      await flushPromises();
      expect(asked.answer).toHaveBeenCalledWith("c2");
      expect(picker(wrapper).props("open")).toBe(false);

      wrapper.findComponent(Sheet).vm.$emit("sendTo", "c2", { kind: "text", text: "# Title" });
      await flushPromises();
      expect(nav.replace).toHaveBeenCalledWith("/chat/c2");
      expect(nav.back).not.toHaveBeenCalled();
      expect(takeSend("c2")).toEqual({ kind: "text", text: "# Title" });
    });

    it("opens the chat picked with nothing waiting when the plugin sent it by itself", async () => {
      const wrapper = await page();
      wrapper.findComponent(Sheet).vm.$emit("sendTo", "c2", null);
      await flushPromises();
      expect(nav.replace).toHaveBeenCalledWith("/chat/c2");
      expect(takeSend("c2")).toBeNull();
    });

    it("answers nobody when the picker is dismissed, and when the page is left", async () => {
      const wrapper = await page();
      const first = ask();
      wrapper.findComponent(Sheet).vm.$emit("pick", first);
      await flushPromises();
      picker(wrapper).vm.$emit("dismiss");
      await flushPromises();
      expect(first.answer).toHaveBeenCalledWith(null);
      expect(picker(wrapper).props("open")).toBe(false);

      const second = ask();
      wrapper.findComponent(Sheet).vm.$emit("pick", second);
      await flushPromises();
      hooks(wrapper, "onIonViewWillLeave");
      await flushPromises();
      expect(second.answer).toHaveBeenCalledWith(null);
      expect(picker(wrapper).props("open")).toBe(false);
      expect(nav.replace).not.toHaveBeenCalled();
    });

    it("leads to adding a contact when there is nobody to send to", async () => {
      const wrapper = await page();
      const asked = ask();
      wrapper.findComponent(Sheet).vm.$emit("pick", asked);
      await flushPromises();
      picker(wrapper).vm.$emit("add");
      await flushPromises();
      expect(asked.answer).toHaveBeenCalledWith(null);
      expect(nav.push).toHaveBeenCalledWith("/add-contact");
    });
  });

  it("is an Ionic page: a header with its back button and the plugin's name, then the plugin", () => {
    expect(pageShape(mount(PluginPage, { shallow: true }), ["permission-ask", "contact-picker"])).toEqual(["ion-header", "ion-content"]);
  });
});
