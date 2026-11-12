import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import CirclePage from "./CirclePage.vue";
import CircleThread from "../components/CircleThread.vue";
import { pageShape } from "../__tests__/page-shape";
import { seed } from "../__tests__/seed";
import { store } from "../core";

vi.mock("vue-router", () => ({ useRoute: () => ({ params: { id: "circle1" } }), useRouter: () => ({ push: vi.fn() }) }));

const hooks = (wrapper: { vm: unknown }, name: string) =>
  ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>)[name] ?? []).forEach((hook) => hook());
const enterView = (wrapper: { vm: unknown }) => {
  hooks(wrapper, "onIonViewWillEnter");
  hooks(wrapper, "onIonViewDidEnter");
};

describe("CirclePage", () => {
  it("shows the circle from the route, with a way back", () => {
    const thread = mount(CirclePage, { shallow: true }).findComponent(CircleThread);
    expect(thread.props("circleId")).toBe("circle1");
    expect(thread.props("showBack")).toBe(true);
  });

  it("tells the circle whether it is on screen", async () => {
    const wrapper = mount(CirclePage, { shallow: true });
    expect(wrapper.findComponent(CircleThread).props("active")).toBe(true);
    hooks(wrapper, "onIonViewWillLeave");
    await wrapper.vm.$nextTick();
    expect(wrapper.findComponent(CircleThread).props("active")).toBe(false);
    enterView(wrapper);
    await wrapper.vm.$nextTick();
    expect(wrapper.findComponent(CircleThread).props("active")).toBe(true);
  });

  it("stays off screen under a page opened over it while it still came in", async () => {
    const wrapper = mount(CirclePage, { shallow: true });
    hooks(wrapper, "onIonViewWillEnter");
    hooks(wrapper, "onIonViewWillLeave");
    hooks(wrapper, "onIonViewDidEnter");
    await wrapper.vm.$nextTick();
    expect(wrapper.findComponent(CircleThread).props("active")).toBe(false);
  });

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
    expect(pageShape(wrapper, ["hook-frames", "permission-ask"])).toEqual(["ion-header", "ion-content", "ion-footer"]);
  });
});
