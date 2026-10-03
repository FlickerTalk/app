import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import CircleThread from "./CircleThread.vue";
import MessageBubble from "./MessageBubble.vue";
import EmojiPicker from "./EmojiPicker.vue";
import { calls, seed } from "../__tests__/seed";
import { store, type Circle } from "../core";
import { defineComponent, h } from "vue";
import { startViewportFit } from "../viewport";
import { setLocale } from "../i18n";

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
});
