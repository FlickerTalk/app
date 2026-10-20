import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { IonLabel, IonToggle } from "@ionic/vue";
import { defineComponent, h } from "vue";
import CircleInfoPage from "./CircleInfoPage.vue";
import HookFrames from "../components/HookFrames.vue";
import PluginSettingsGroup from "../components/PluginSettingsGroup.vue";
import { pageShape } from "../__tests__/page-shape";
import { calls, seed } from "../__tests__/seed";
import { store, type Circle, type PluginView } from "../core";
import { installed, premiumLocked } from "../plugins";

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

// Each page goes when its test ends, as a page does: the hooks of a stale one must not run on.
enableAutoUnmount(afterEach);

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
  // 2026-10-20 (plan of the translator §8.2d, screen G): the circle gets the contact's native group
  // for each tool with a settings hook (the Translator's language and switches), drawn from what it
  // declares; its hook runs out of sight, in a frame the page hosts, for the circle.
  describe("tools' settings for the circle", () => {
    const TRANSLATOR: PluginView = {
      id: "com.flickertalk.translate",
      name: "Translator",
      version: "1.1.0",
      kind: "tool",
      asks: { network: [], messages: true, send: "propose", translate: true },
      granted: { network: [], messages: true, send: "propose", translate: true },
      installedAt: 1,
      hooks: {
        message: { label: "Translate", icon: "language-outline" },
        settings: {
          label: "Translation",
          icon: "language-outline",
          fields: [
            { key: "lang", type: "select", label: "Language" },
            { key: "receive", type: "toggle", label: "Translate what I receive" },
          ],
        },
      },
    };
    const PLAIN: PluginView = { ...TRANSLATOR, id: "com.example.plain", hooks: { message: TRANSLATOR.hooks!.message } };
    /** What each run of the hook was handed, and what the tool answers. */
    let handed: Record<string, unknown>[] = [];
    let answer: (hook: Record<string, unknown>) => unknown = () => null;
    const HookSheet = defineComponent({
      name: "PluginSheet",
      props: { plugin: { type: Object, default: undefined }, contact: { type: String, default: "" }, hidden: Boolean, sending: { type: String, default: "" }, session: { type: String, default: undefined }, fill: Boolean },
      setup(_props, { expose }) {
        expose({ runHook: async (hook: Record<string, unknown>) => (handed.push(hook), answer(hook)) });
        return () => h("div");
      },
    });
    let fallback: (command: string, args?: Record<string, unknown>) => Promise<unknown>;
    const internals = () => (window as unknown as { __TAURI_INTERNALS__: { invoke: typeof fallback } }).__TAURI_INTERNALS__;

    beforeEach(() => {
      handed = [];
      answer = () => null;
      fallback = internals().invoke;
      internals().invoke = (command, args) => {
        if (command === "core_hook_sample") {
          calls.push([command, args]);
          return Promise.resolve("on se voit samedi ?");
        }
        if (command === "core_plugin_chat") return Promise.resolve("key");
        return fallback(command, args);
      };
    });
    afterEach(() => {
      internals().invoke = fallback;
      installed.value = [];
      premiumLocked.value = false;
    });

    it("draws a group for each tool with a settings hook, after the members and before the way out, with the frames to run them", async () => {
      installed.value = [PLAIN, TRANSLATOR, { ...TRANSLATOR, id: "com.example.game", kind: "game" }];
      const wrapper = mount(CircleInfoPage, { shallow: true });
      await flushPromises();
      const groups = wrapper.findAllComponents(PluginSettingsGroup);
      expect(groups.map((one) => (one.props("plugin") as PluginView).id)).toEqual([TRANSLATOR.id]);
      const lastMember = wrapper.findAll("[data-test='circle-member']").at(-1)!.element;
      expect(lastMember.compareDocumentPosition(groups[0].element) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(groups[0].element.compareDocumentPosition(wrapper.find("[data-test='circle-leave']").element) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(wrapper.findComponent(HookFrames).props("contact"), "the circle is the conversation").toBe("circle1");
      expect(wrapper.findComponent(HookFrames).props("session")).toBeUndefined();
      expect(groups[0].props("hooks")).toBe(wrapper.findComponent(HookFrames).props("hooks"));
    });

    it("draws none for a locked tool", async () => {
      installed.value = [TRANSLATOR];
      premiumLocked.value = true;
      const wrapper = mount(CircleInfoPage, { shallow: true });
      await flushPromises();
      expect(wrapper.findAllComponents(PluginSettingsGroup)).toHaveLength(0);
    });

    it("runs the settings hook out of sight as the page opens, with the circle's latest texts", async () => {
      answer = () => ({ values: { lang: "fr", receive: false }, options: { lang: [{ value: "fr", label: "French (detected)" }] } });
      installed.value = [TRANSLATOR];
      const wrapper = mount(CircleInfoPage, { global: { stubs: { PluginSheet: HookSheet, IonIcon: true } } });
      await vi.waitFor(() => expect(handed).toEqual([{ name: "settings", text: "on se voit samedi ?", chat: "key", ref: null, lang: "en", manual: false }]));
      expect(calls).toContainEqual(["core_hook_sample", { plugin: TRANSLATOR.id, conversation: "circle1" }]);
      await flushPromises();
      expect(wrapper.find(`[data-test='settings-${TRANSLATOR.id}'] [data-test='setting-receive']`).exists()).toBe(true);
    });

    it("keeps the switches the tool answers for the circle when one is changed", async () => {
      answer = (hook) =>
        hook.manual
          ? { values: { lang: "fr", receive: true }, auto: { message: true, composer: false }, lang: "fr" }
          : { values: { lang: "fr", receive: false }, options: { lang: [{ value: "fr", label: "French (detected)" }] } };
      installed.value = [TRANSLATOR];
      const wrapper = mount(CircleInfoPage, { global: { stubs: { PluginSheet: HookSheet, IonIcon: true } } });
      await vi.waitFor(() => expect(handed).toHaveLength(1));
      await flushPromises();
      const receive = wrapper.findAllComponents(IonToggle).find((one) => one.attributes("data-test") === "setting-receive")!;
      receive.vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: true } }));
      await vi.waitFor(() => expect(calls.filter(([command]) => command === "core_chat_hooks_set")).toHaveLength(1));
      expect(handed[1]).toMatchObject({ name: "settings", manual: true, values: { lang: "fr", receive: true } });
      expect(calls).toContainEqual(["core_chat_hooks_set", { conversation: "circle1", plugin: TRANSLATOR.id, autoMessage: true, autoComposer: false, lang: "fr" }]);
    });

    it("runs the frames in the circle's hidden session when it lives in one", async () => {
      store.circles = [];
      store.sessions = [{ id: "s7", chats: [], requests: [], circles: [friends()] } as unknown as (typeof store.sessions)[number]];
      installed.value = [TRANSLATOR];
      const wrapper = mount(CircleInfoPage, { shallow: true });
      await flushPromises();
      expect(wrapper.findComponent(HookFrames).props("session")).toBe("s7");
    });
  });

  it("is an Ionic page: a header with its back button, then the circle's settings", () => {
    // The tools' hooks' frames (2026-10-20) are overlays: fixed windows out of sight and their sheet.
    expect(pageShape(mount(CircleInfoPage, { shallow: true }), ["hook-frames"])).toEqual(["ion-header", "ion-content"]);
  });
});
