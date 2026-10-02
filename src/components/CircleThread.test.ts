import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import CircleThread from "./CircleThread.vue";
import MessageBubble from "./MessageBubble.vue";
import { calls, seed } from "../__tests__/seed";
import { store, type Circle } from "../core";
import { defineComponent, h } from "vue";
import { startViewportFit } from "../viewport";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));

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

  it("loads the circle's messages from the core and marks them read", async () => {
    mount(CircleThread, { props: { circleId: "circle1" }, shallow: true });
    await flushPromises();
    expect(calls).toContainEqual(["core_circle_messages", { circle: "circle1", limit: 200 }]);
    expect(calls).toContainEqual(["core_circle_mark_read", { circle: "circle1" }]);
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
