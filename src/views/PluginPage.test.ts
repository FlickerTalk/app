import { describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h, reactive } from "vue";
import { setLocale } from "../i18n";

const nav = vi.hoisted(() => ({ push: vi.fn(), back: vi.fn() }));
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
import { installed, premiumLocked } from "../plugins";
import PluginSheet from "../components/PluginSheet.vue";

describe("PluginPage", () => {
  // Found on a real phone (2026-09-27): a reminder tapped while its plugin is on screen only
  // changes the query of the same page, which has to pass the new one on.
  it("follows the reminder in the address, not only the first one", async () => {
    route.value = reactive({ params: { id: "com.flickertalk.notes" }, query: { reminder: "r1" } });
    const wrapper = mount(PluginPage, { shallow: true });
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).props("reminder")).toBe("r1");

    route.value.query = { reminder: "r2" };
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).props("reminder")).toBe("r2");
  });

  // 2026-10-01 (§108): a reminder set inside a hidden session opens the plugin in it; from
  // Settings, in none.
  it("opens the plugin in the session the address names, or in none", async () => {
    route.value = reactive({ params: { id: "com.flickertalk.notes" }, query: { reminder: "r1", session: "s1" } });
    const wrapper = mount(PluginPage, { shallow: true });
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).props("session")).toBe("s1");

    route.value.query = {};
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).props("session")).toBeUndefined();
  });

  // 2026-10-02 (plan of the catalogue's translations): its title in the phone's language.
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

  // 2026-10-02: leaving the page closes the plugin through the sheet, so it can say goodbye while
  // the page goes; back on the page after that, it is opened afresh.
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

    // Back before it finished saying goodbye: once it has, it is opened again in sight.
    it("opens the plugin afresh at once when it said goodbye with the page on screen", async () => {
      const wrapper = await page();
      hooks(wrapper, "onIonViewWillLeave");
      hooks(wrapper, "onIonViewWillEnter");
      wrapper.findComponent(Sheet).vm.$emit("closed");
      await flushPromises();
      expect(mounted).toBe(2);
    });
  });

  // Ioan, 2026-10-08: a tool opened on its own (from Settings, a reminder) while the tools are
  // locked shows the lock and the way to the subscription, never its frame. A game still opens.
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
});
