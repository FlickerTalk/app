import { describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { reactive } from "vue";

const route = vi.hoisted(() => ({ value: null as unknown as { params: Record<string, string>; query: Record<string, string> } }));
vi.mock("vue-router", async (importOriginal) => ({
  ...(await importOriginal<typeof import("vue-router")>()),
  useRoute: () => route.value,
  useRouter: () => ({ push: vi.fn(), back: vi.fn() }),
}));
vi.mock("../plugins", async () => {
  const { ref } = await import("vue");
  return {
    installed: ref([{ id: "com.flickertalk.notes", name: "Notes" }]),
    refreshPlugins: vi.fn(),
  };
});

import PluginPage from "./PluginPage.vue";
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
});
