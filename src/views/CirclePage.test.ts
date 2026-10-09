import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import CirclePage from "./CirclePage.vue";
import CircleThread from "../components/CircleThread.vue";
import { pageShape } from "../__tests__/page-shape";
import { seed } from "../__tests__/seed";
import { store } from "../core";

vi.mock("vue-router", () => ({ useRoute: () => ({ params: { id: "circle1" } }), useRouter: () => ({ push: vi.fn() }) }));

/** Runs the Ionic page hooks the page registered under this name. */
const hooks = (wrapper: { vm: unknown }, name: string) =>
  ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>)[name] ?? []).forEach((hook) => hook());

describe("CirclePage", () => {
  it("shows the circle from the route, with a way back", () => {
    const thread = mount(CirclePage, { shallow: true }).findComponent(CircleThread);
    expect(thread.props("circleId")).toBe("circle1");
    expect(thread.props("showBack")).toBe(true);
  });

  // app#80 (2026-10-03): Ionic keeps the circle mounted under its settings; what it left open lets
  // go of Android's back button there, and takes it again when the circle is back on screen.
  it("tells the circle whether it is on screen", async () => {
    const wrapper = mount(CirclePage, { shallow: true });
    expect(wrapper.findComponent(CircleThread).props("active")).toBe(true);
    hooks(wrapper, "onIonViewWillLeave");
    await wrapper.vm.$nextTick();
    expect(wrapper.findComponent(CircleThread).props("active")).toBe(false);
    hooks(wrapper, "onIonViewDidEnter");
    await wrapper.vm.$nextTick();
    expect(wrapper.findComponent(CircleThread).props("active")).toBe(true);
  });

  // Ionic's own shape (2026-10-09), as the chat's page: the circle's header, content and footer are
  // the page's own children, where Ionic's transitions look for them, with no frame of ours between.
  it("is an Ionic page: the circle's header, content and footer are the page's own children", () => {
    seed();
    store.circles = [
      {
        id: "circle1",
        name: "Friends",
        hue: 120,
        members: [{ id: "ft_me", name: "Me", admin: true, me: true }],
        admin: true,
        adminsOnly: false,
        left: false,
        unread: 0,
        time: "10:02",
        preview: "",
        lastMine: false,
        lastSender: "",
        status: "delivered",
        messages: [],
      },
    ];
    const wrapper = mount(CirclePage, { shallow: true, global: { stubs: { CircleThread: false } } });
    expect(pageShape(wrapper)).toEqual(["ion-header", "ion-content", "ion-footer"]);
  });
});
