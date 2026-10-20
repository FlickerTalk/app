import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import CircleThread from "./CircleThread.vue";
import MessageBubble from "./MessageBubble.vue";
import EmojiPicker from "./EmojiPicker.vue";
import { IonTextarea } from "@ionic/vue";
import { calls, seed } from "../__tests__/seed";
import { circle as circleOf, store, type Circle } from "../core";
import { defineComponent, h } from "vue";
import { startViewportFit } from "../viewport";
import { setLocale } from "../i18n";
import { premiumLocked, refreshPlugins } from "../plugins";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));
// Android's back button: the handler the app listens with while something is open on top.
const back = vi.hoisted(() => ({ handler: null as null | (() => void), listener: null as null | object }));
vi.mock("@tauri-apps/api/app", () => ({
  onBackButtonPress: async (handler: () => void) => {
    const listener = {};
    back.handler = handler;
    back.listener = listener;
    return { unregister: async () => void (back.listener === listener && ((back.handler = null), (back.listener = null))) };
  },
}));

function friends(overrides: Partial<Circle> = {}): Circle {
  return {
    id: "circle1",
    name: "Friends",
    hue: 120,
    members: [
      { id: "ft_me", name: "Me", admin: true, me: true },
      { id: "ft_bob", name: "Bob", admin: false, me: false },
      { id: "ft_carol", name: "Carol", admin: false, me: false },
    ],
    admin: true,
    adminsOnly: false,
    left: false,
    unread: 1,
    time: "10:02",
    preview: "dinner on friday?",
    lastMine: false,
    lastSender: "Bob",
    status: "delivered",
    messages: [
      { id: "e1", mine: true, text: "Friends", time: "10:00", kind: "created", sender: "ft_me", senderName: "Me" },
      { id: "m1", mine: false, text: "dinner on friday?", time: "10:02", status: "delivered", kind: "text", sender: "ft_bob", senderName: "Bob" },
      { id: "m2", mine: true, text: "yes!", time: "10:03", status: "sent", kind: "text", sender: "ft_me", senderName: "Me" },
    ],
    ...overrides,
  };
}

// Circles (2026-09-27): a conversation of many; who said what is written over each bubble that
// is not ours, and what happened is a line between them.

/** A visual viewport the test moves, as the on-screen keyboard would (2026-09-29). */
class FakeViewport extends EventTarget {
  height = 900;
  offsetTop = 0;
  move(height: number) {
    this.height = height;
    this.dispatchEvent(new Event("resize"));
  }
}
/** `ion-content` whose element hands out a scroller the test can read. */
const contentWith = (scroller: { scrollHeight: number; clientHeight: number; scrollTop: number }) =>
  defineComponent({
    mounted() {
      (this.$el as { getScrollElement?: () => Promise<unknown> }).getScrollElement = async () => scroller;
    },
    render() {
      return h("div", this.$slots.default?.());
    },
  });

// Each thread goes when its test ends, as a page does: a stale one must not mark the next one read.
enableAutoUnmount(afterEach);

describe("CircleThread", () => {
  beforeEach(() => {
    push.mockClear();
    seed();
    store.circles = [friends()];
  });

  // Seen in Arabic (2026-10-02): the circle's name reads in its own direction, not in the app's.
  it("shows the circle's name in its own direction", () => {
    store.circles = [friends({ name: "Amigos!" })];
    const wrapper = mount(CircleThread, { props: { circleId: "circle1" }, shallow: true });
    expect(wrapper.find(".ft-peer__name").attributes("dir")).toBe("auto");
  });

  it("shows the texts as bubbles, with who said them, and what happened as a line", () => {
    const wrapper = mount(CircleThread, { props: { circleId: "circle1" }, shallow: true });
    const bubbles = wrapper.findAllComponents(MessageBubble);
    expect(bubbles).toHaveLength(2);
    expect(bubbles[0].props("sender")).toBe("Bob");
    const events = wrapper.findAll("[data-test='circle-event']");
    expect(events).toHaveLength(1);
    expect(events[0].text()).toBe("You created the circle «Friends»");
    expect(wrapper.text()).toContain("Members: 3");
  });

  // Found on the phones (2026-10-03, app#79): "Tú creó el círculo" in Spanish. What I did is told
  // in the first person, with its own sentence in each language, never "you" as a third person.
  it("tells what I did in the first person, in the phone's language", async () => {
    const me = { mine: true, time: "10:00", sender: "ft_me", senderName: "Me" };
    store.circles = [
      friends({
        messages: [
          { ...me, id: "e1", text: "Friends", kind: "created" },
          { ...me, id: "e2", text: "Friends B", kind: "renamed" },
          { ...me, id: "e3", text: "Dave", kind: "joined" },
          { ...me, id: "e4", text: "Carol", kind: "removed" },
          { ...me, id: "e5", text: "Me", kind: "left" },
          { id: "e6", mine: false, text: "Friends C", time: "10:05", kind: "renamed", sender: "ft_bob", senderName: "Bob" },
        ],
      }),
    ];
    await setLocale("es");
    try {
      const wrapper = mount(CircleThread, { props: { circleId: "circle1" }, shallow: true });
      expect(wrapper.findAll("[data-test='circle-event']").map((line) => line.text())).toEqual([
        "Creaste el círculo «Friends»",
        "Cambiaste el nombre del círculo a «Friends B»",
        "Añadiste a Dave",
        "Sacaste a Carol",
        "Saliste del círculo",
        "Bob cambió el nombre del círculo a «Friends C»",
      ]);
    } finally {
      await setLocale("en");
    }
  });

  it("loads the circle's messages from the core and marks them read", async () => {
    mount(CircleThread, { props: { circleId: "circle1" }, shallow: true });
    await flushPromises();
    expect(calls).toContainEqual(["core_circle_messages", { circle: "circle1", limit: 200 }]);
    expect(calls).toContainEqual(["core_circle_mark_read", { circle: "circle1" }]);
  });

  // architecture#21, as in a chat: the page stays mounted with the app behind Android's home
  // screen, and what arrives there must not leave the list without its unread count.
  describe("read only while on screen", () => {
    let visibility: DocumentVisibilityState = "visible";
    const setVisibility = (state: DocumentVisibilityState) => {
      visibility = state;
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });
    };
    afterEach(() => Reflect.deleteProperty(document, "visibilityState"));
    const arrive = () =>
      store.circles[0].messages.push({ id: "m9", mine: false, text: "anyone?", time: "10:05", status: "delivered", kind: "text", sender: "ft_bob", senderName: "Bob" });
    const markedRead = () => calls.some(([command]) => command === "core_circle_mark_read");

    it("does not mark read what arrives while the app is in the background", async () => {
      mount(CircleThread, { props: { circleId: "circle1" }, shallow: true });
      await flushPromises();
      setVisibility("hidden");
      calls.length = 0;
      arrive();
      await flushPromises();
      expect(markedRead()).toBe(false);
    });

    it("marks it read once the app is back on the screen", async () => {
      mount(CircleThread, { props: { circleId: "circle1" }, shallow: true });
      await flushPromises();
      setVisibility("hidden");
      arrive();
      await flushPromises();
      calls.length = 0;
      setVisibility("visible");
      document.dispatchEvent(new Event("visibilitychange"));
      await flushPromises();
      expect(calls).toContainEqual(["core_circle_mark_read", { circle: "circle1" }]);
    });

    it("does not mark the circle read when it opens with the app in the background", async () => {
      setVisibility("hidden");
      mount(CircleThread, { props: { circleId: "circle1" }, shallow: true });
      await flushPromises();
      expect(markedRead()).toBe(false);
    });
  });

  it("sends a text to the circle", async () => {
    const wrapper = mount(CircleThread, { props: { circleId: "circle1" }, shallow: true });
    await flushPromises();
    await wrapper.findComponent({ name: "IonTextarea" }).vm.$emit("update:modelValue", "count me in");
    await wrapper.find("[data-test='circle-send']").trigger("click");
    expect(calls).toContainEqual(["core_circle_send", { circle: "circle1", text: "count me in" }]);
  });

  it("opens the circle's settings from its header", async () => {
    const wrapper = mount(CircleThread, { props: { circleId: "circle1" }, shallow: true });
    await wrapper.find("[data-test='circle-peer']").trigger("click");
    expect(push).toHaveBeenCalledWith("/circle/circle1/info");
  });

  it("only reads when only the admins write and this phone is none", () => {
    store.circles = [friends({ admin: false, adminsOnly: true })];
    const wrapper = mount(CircleThread, { props: { circleId: "circle1" }, shallow: true });
    expect(wrapper.find("[data-test='circle-read-only']").exists()).toBe(true);
    expect(wrapper.find("[data-test='circle-send']").exists()).toBe(false);
  });

  it("says so when this phone is no longer in the circle", () => {
    store.circles = [friends({ left: true })];
    const wrapper = mount(CircleThread, { props: { circleId: "circle1" }, shallow: true });
    expect(wrapper.find("[data-test='circle-left']").text()).toBe("You are no longer in this circle");
    expect(wrapper.find("[data-test='circle-send']").exists()).toBe(false);
  });

  // Found on the phones (2026-10-03, app#80): Back did nothing with the emoji open, as in a chat it
  // closes them. It closes the panel, and only that: the circle stays on screen.
  it("closes the emoji with the back button, and stays in the circle", async () => {
    const wrapper = mount(CircleThread, { props: { circleId: "circle1" }, shallow: true });
    await flushPromises();
    expect(back.handler).toBeNull();

    await wrapper.find("[data-test='open-emoji']").trigger("click");
    await flushPromises();
    expect(wrapper.findComponent(EmojiPicker).exists()).toBe(true);
    expect(back.handler).not.toBeNull();

    back.handler?.();
    await flushPromises();
    expect(wrapper.findComponent(EmojiPicker).exists()).toBe(false);
    expect(back.handler).toBeNull();
    expect(push).not.toHaveBeenCalled();
  });

  // Ionic keeps the circle mounted under its settings: the emoji it left open must not take Back
  // there, and take it again when the circle is back on screen.
  it("lets go of the back button while the circle is not on screen", async () => {
    const wrapper = mount(CircleThread, { props: { circleId: "circle1", active: true }, shallow: true });
    await wrapper.find("[data-test='open-emoji']").trigger("click");
    await flushPromises();
    expect(back.handler).not.toBeNull();

    await wrapper.setProps({ active: false });
    await flushPromises();
    expect(back.handler).toBeNull();

    await wrapper.setProps({ active: true });
    await flushPromises();
    expect(back.handler).not.toBeNull();
  });

  // The keyboard shrinks the conversation: the last message stays in sight above the composer.
  it("keeps the last message in sight when the keyboard opens", async () => {
    const viewport = new FakeViewport();
    Object.defineProperty(window, "visualViewport", { value: viewport, configurable: true });
    Object.defineProperty(window, "innerHeight", { value: 900, configurable: true });
    const stop = startViewportFit(window, document.documentElement);
    const scroller = { scrollHeight: 2000, clientHeight: 800, scrollTop: 1200 };
    const wrapper = mount(CircleThread, {
      props: { circleId: "circle1" },
      shallow: true,
      global: { stubs: { IonContent: contentWith(scroller) } },
    });
    await flushPromises();
    viewport.move(560);
    expect(scroller.scrollTop).toBe(2000);

    // Once it is gone, it no longer moves anything.
    wrapper.unmount();
    scroller.scrollTop = 1200;
    viewport.move(900);
    viewport.move(560);
    expect(scroller.scrollTop).toBe(1200);
    stop();
    document.documentElement.removeAttribute("style");
  });
  // 2026-10-20 (plan of the translator §8.2d, screen G of the approved mockup): a circle gets what a
  // 1:1 chat gets. Each received text offers the message hook's link inside its bubble (with who
  // said it over it) while the circle's language is not the app's; the tool runs out of sight and its
  // answer takes the text's place. With the circle's switches on (set in its info page), a chip under
  // the header says so, what arrives is translated by itself, and what is sent goes in the circle's
  // language with the user's own words kept under it. The tools' frames are stubbed: what each is
  // handed, and what the test says it answers.
  describe("the chat's hooks", () => {
    const ID = "com.flickertalk.translate";
    const KEY = "k".repeat(43);
    const TRANSLATOR = {
      id: ID,
      name: "Translator",
      version: "1.1.0",
      kind: "tool",
      asks: { network: [], messages: true, send: "propose", translate: true },
      granted: { network: [], messages: true, send: "propose", translate: true },
      installedAt: 1,
      hooks: {
        message: { label: "Translate", icon: "language-outline" },
        composer: { label: "Translate", icon: "language-outline" },
      },
    };
    type Hook = { name: string; text: string; chat: string; ref: string | null; lang: string; manual: boolean; answer?: boolean };
    let answer: (hook: Hook) => unknown = () => null;
    const sheets: { props: Record<string, unknown>; runHook: ReturnType<typeof vi.fn> }[] = [];
    const HookSheet = defineComponent({
      name: "PluginSheet",
      props: {
        plugin: { type: Object, default: undefined },
        contact: { type: String, default: "" },
        hidden: Boolean,
        sending: { type: String, default: "nothing" },
        session: { type: String, default: undefined },
        fill: Boolean,
      },
      emits: ["text", "attach", "done", "closed", "needs", "openChat", "shown", "refused", "pick", "sendTo"],
      setup(props, { expose }) {
        const runHook = vi.fn(async (hook: Hook) => answer(hook));
        sheets.push({ props, runHook });
        expose({ runHook });
        return () => h("div", { "data-test": "hook-frame" });
      },
    });
    /** Ionic's sheet shows nothing in happy-dom: stubbed to its content while open. */
    const IonModalStub = defineComponent({
      name: "IonModal",
      props: { isOpen: Boolean },
      setup(props, { slots }) {
        return () => (props.isOpen ? h("div", slots.default?.()) : null);
      },
    });
    const stubs = { IonIcon: true, IonModal: IonModalStub, PluginSheet: HookSheet, EmojiPicker: true };
    let view: Record<string, unknown> = TRANSLATOR;
    let flags: unknown[] = [];
    /** What tools said under the circle's messages, by message id, as the core gives them back. */
    let notes: Record<string, { plugin: string; text: string }[]> = {};
    /** What the core answers for a text sent to the circle: the id of the message it made. */
    let sentId: string | undefined;

    beforeEach(() => {
      sheets.length = 0;
      answer = () => null;
      view = JSON.parse(JSON.stringify(TRANSLATOR));
      flags = [];
      notes = {};
      sentId = undefined;
      const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown> } }).__TAURI_INTERNALS__;
      const fallback = internals.invoke;
      internals.invoke = async (command, args) => {
        if (command === "core_plugins") return [view];
        if (command === "core_chat_hooks") return flags;
        if (command === "core_plugin_chat") {
          calls.push([command, args]);
          return KEY;
        }
        if (command === "core_plugin_ref") return "ref_1";
        if (command === "core_plugin_grant") view = { ...view, granted: args?.granted };
        if (command === "core_hook_sample") {
          calls.push([command, args]);
          return "dinner on friday?";
        }
        if (command === "core_circle_send") {
          calls.push([command, args]);
          return sentId;
        }
        if (command === "core_circle_messages") {
          calls.push([command, args]);
          // From wherever the circle lives (the main list or a hidden session), with the notes kept.
          return (circleOf(String(args?.circle))?.messages ?? []).map((one) => ({
            id: one.id,
            outgoing: one.mine,
            sender: one.sender,
            senderName: one.senderName,
            kind: one.kind,
            text: one.text,
            sentAt: 0,
            state: one.status ?? "delivered",
            ...(notes[one.id] ? { notes: notes[one.id] } : {}),
          }));
        }
        return fallback(command, args);
      };
    });
    afterEach(() => {
      premiumLocked.value = false;
    });

    async function thread(props: Record<string, unknown> = {}) {
      await refreshPlugins();
      const wrapper = mount(CircleThread, { props: { circleId: "circle1", ...props }, global: { stubs } });
      await flushPromises();
      return wrapper;
    }
    const kept = (command: string) => calls.filter(([one]) => one === command).map(([, args]) => args);
    const frames = () => sheets.filter((one) => one.props.hidden);
    /** The bubbles offering the message hook's link, by message id. */
    const offering = (wrapper: ReturnType<typeof mount>) =>
      wrapper.findAllComponents(MessageBubble).filter((one) => one.find("[data-test='translate']").exists()).map((one) => one.props("message").id);

    it("offers the hook's link under each text others said, not under mine nor what happened", async () => {
      const wrapper = await thread();
      expect(offering(wrapper)).toEqual(["m1"]);
      expect(wrapper.find("[data-test='translating']").exists(), "nothing runs by itself").toBe(false);
    });

    it("offers it while the circle is in another language, and not in the app's", async () => {
      flags = [{ plugin: ID, autoMessage: false, autoComposer: false, lang: "fr" }];
      expect(offering(await thread())).toEqual(["m1"]);
      flags = [{ plugin: ID, autoMessage: false, autoComposer: false, lang: "en" }];
      expect(offering(await thread())).toEqual([]);
    });

    it("offers nothing without a tool bound to messages", async () => {
      view = { ...TRANSLATOR, hooks: { composer: TRANSLATOR.hooks.composer } };
      expect(offering(await thread())).toEqual([]);
    });

    it("runs the message hook out of sight on a tap, in a frame of the circle's, and keeps its answer", async () => {
      answer = (hook) => (hook.name === "message" ? { text: "¿cenamos el viernes?" } : null);
      const wrapper = await thread();
      await wrapper.findAllComponents(MessageBubble)[0].find("[data-test='translate']").trigger("click");
      await flushPromises();
      expect(frames()).toHaveLength(1);
      expect(frames()[0].props.contact, "the circle is the conversation").toBe("circle1");
      expect(frames()[0].runHook).toHaveBeenCalledWith({ name: "message", text: "dinner on friday?", chat: KEY, ref: "ref_1", lang: "en", manual: true });
      expect(kept("core_plugin_chat")).toEqual([{ plugin: ID, contact: "circle1" }]);
      expect(kept("core_message_note_set")).toEqual([{ message: "m1", plugin: ID, text: "¿cenamos el viernes?" }]);
    });

    it("shows a kept answer in the text's place, with who said it, and the original one tap away", async () => {
      notes = { m1: [{ plugin: ID, text: "¿cenamos el viernes?" }] };
      const wrapper = await thread();
      const bubble = wrapper.findAllComponents(MessageBubble)[0];
      expect(bubble.props("sender")).toBe("Bob");
      expect(bubble.text()).toContain("¿cenamos el viernes?");
      expect(bubble.text()).not.toContain("dinner on friday?");
      await bubble.find("[data-test='show-original']").trigger("click");
      expect(bubble.text()).toContain("dinner on friday?");
    });

    it("asks first when the tool may not read the message yet, and runs only after a yes", async () => {
      view = { ...TRANSLATOR, granted: { ...TRANSLATOR.granted, messages: false } };
      answer = () => ({ text: "hola" });
      const wrapper = await thread();
      await wrapper.findAllComponents(MessageBubble)[0].find("[data-test='translate']").trigger("click");
      await flushPromises();
      expect(wrapper.find("[data-test='permission-ask']").html()).toContain("Read what you send it");
      expect(frames()).toHaveLength(0);
      await wrapper.find("[data-test='permission-allow']").trigger("click");
      await flushPromises();
      expect(kept("core_plugin_grant")).toEqual([{ plugin: ID, granted: expect.objectContaining({ messages: true }) }]);
      expect(frames()[0].runHook).toHaveBeenCalledWith(expect.objectContaining({ name: "message", manual: true }));
      expect(kept("core_message_note_set")).toHaveLength(1);
    });

    it("runs nothing and grants nothing after a no", async () => {
      view = { ...TRANSLATOR, granted: { ...TRANSLATOR.granted, messages: false } };
      const wrapper = await thread();
      await wrapper.findAllComponents(MessageBubble)[0].find("[data-test='translate']").trigger("click");
      await flushPromises();
      await wrapper.find("[data-test='permission-cancel']").trigger("click");
      await flushPromises();
      expect(kept("core_plugin_grant")).toEqual([]);
      expect(frames()).toHaveLength(0);
    });

    it("leads a locked tool's link to Premium", async () => {
      const wrapper = await thread();
      premiumLocked.value = true;
      await wrapper.findAllComponents(MessageBubble)[0].find("[data-test='translate']").trigger("click");
      await flushPromises();
      expect(push).toHaveBeenCalledWith("/tabs/settings#premium");
      expect(frames()).toHaveLength(0);
    });

    it("says under the header that it translates by itself, into what, and leads to the circle's info", async () => {
      flags = [{ plugin: ID, autoMessage: true, autoComposer: false, lang: "fr" }];
      const wrapper = await thread();
      const chip = wrapper.find("[data-test='translating']");
      // happy-dom reads no text through Ionic's chip and label: their markup says it.
      expect(chip.html()).toContain("Translating · French");
      await chip.trigger("click");
      expect(push).toHaveBeenCalledWith("/circle/circle1/info");
    });

    it("sends in the circle's language with its composer switch on, and keeps my words under it", async () => {
      flags = [{ plugin: ID, autoMessage: false, autoComposer: true, lang: "fr" }];
      sentId = "m9";
      answer = (hook) => (hook.name === "composer" ? { text: "bonjour" } : null);
      const wrapper = await thread();
      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "hello");
      await flushPromises();
      await wrapper.find("[data-test='circle-send']").trigger("click");
      await flushPromises();
      expect(frames()[0].runHook).toHaveBeenCalledWith(expect.objectContaining({ name: "composer", text: "hello", manual: false }));
      expect(kept("core_circle_send")).toEqual([{ circle: "circle1", text: "bonjour" }]);
      expect(kept("core_message_note_set")).toEqual([{ message: "m9", plugin: ID, text: "hello" }]);
    });

    it("sends my words as written without the switch", async () => {
      const wrapper = await thread();
      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "hello");
      await flushPromises();
      await wrapper.find("[data-test='circle-send']").trigger("click");
      await flushPromises();
      expect(frames()).toHaveLength(0);
      expect(kept("core_circle_send")).toEqual([{ circle: "circle1", text: "hello" }]);
      expect(kept("core_message_note_set")).toEqual([]);
    });

    it("names the circle's language on my texts sent in it", async () => {
      flags = [{ plugin: ID, autoMessage: false, autoComposer: true, lang: "fr" }];
      const wrapper = await thread();
      expect(wrapper.findAllComponents(MessageBubble)[1].props("language")).toBe("French");
    });

    it("translates the texts others said by itself with the circle's message switch on, never what happened", async () => {
      store.circles = [
        friends({
          messages: [
            ...friends().messages,
            { id: "e2", mine: false, text: "Dave", time: "10:04", kind: "joined", sender: "ft_bob", senderName: "Bob" },
            { id: "m3", mine: false, text: "and saturday?", time: "10:05", status: "delivered", kind: "text", sender: "ft_carol", senderName: "Carol" },
          ],
        }),
      ];
      flags = [{ plugin: ID, autoMessage: true, autoComposer: false, lang: "fr" }];
      answer = (hook) => ({ text: `[${hook.text}]` });
      await thread();
      await vi.waitFor(() => expect(kept("core_message_note_set")).toHaveLength(2));
      expect(frames()[0].runHook.mock.calls.map(([hook]) => [hook.text, hook.manual])).toEqual([
        ["and saturday?", false],
        ["dinner on friday?", false],
      ]);
    });

    it("translates nothing by itself while the circle is not on screen", async () => {
      flags = [{ plugin: ID, autoMessage: true, autoComposer: false, lang: "fr" }];
      answer = (hook) => ({ text: `[${hook.text}]` });
      await thread({ active: false });
      await flushPromises();
      expect(frames()).toHaveLength(0);
      expect(kept("core_message_note_set")).toEqual([]);
    });

    // §8.2c/§8.2d: a circle without a language finds it as it opens, from the latest texts others
    // said there, as the core hands them to the tool; what the tool finds is kept for the circle.
    it("finds the circle's language as it opens, from what the core hands the tool for the circle", async () => {
      view = {
        ...TRANSLATOR,
        hooks: { ...TRANSLATOR.hooks, settings: { label: "Translation", icon: "language-outline", fields: [{ key: "lang", type: "select", label: "Language" }] } },
      };
      answer = (hook) => (hook.name === "settings" ? { lang: "fr" } : null);
      await thread();
      await vi.waitFor(() => expect(kept("core_chat_hooks_set")).toHaveLength(1));
      expect(kept("core_hook_sample")).toEqual([{ plugin: ID, conversation: "circle1" }]);
      expect(frames()[0].runHook).toHaveBeenCalledWith({ name: "settings", text: "dinner on friday?", chat: KEY, ref: null, lang: "en", manual: false });
      expect(kept("core_chat_hooks_set")).toEqual([{ conversation: "circle1", plugin: ID, autoMessage: false, autoComposer: false, lang: "fr" }]);
    });

    it("runs the frames in the circle's hidden session when it lives in one", async () => {
      const mine = friends();
      store.circles = [];
      store.sessions = [{ id: "s7", chats: [], requests: [], circles: [mine] } as unknown as (typeof store.sessions)[number]];
      answer = () => ({ text: "hola" });
      const wrapper = await thread();
      await wrapper.findAllComponents(MessageBubble)[0].find("[data-test='translate']").trigger("click");
      await flushPromises();
      expect(frames()[0].props.session).toBe("s7");
    });
  });
});
