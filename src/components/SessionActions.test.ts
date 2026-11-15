import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import SessionActions from "./SessionActions.vue";
import { calls, seed } from "../__tests__/seed";
import { store } from "../core";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));

const actions = () => mount(SessionActions, { props: { session: store.sessions[0] }, shallow: true });

describe("SessionActions", () => {
  beforeEach(() => {
    push.mockClear();
    seed();
  });

  describe("of an open session", () => {
    beforeEach(() => {
      store.sessions = [{ id: "s1", chats: [{ ...store.chats[0], id: "ft_pablo", name: "Pablo", unread: 0 }], requests: [], circles: [] }];
    });

    it("adds a contact to that session, not to the main list", async () => {
      await actions().find("[data-test='session-add']").trigger("click");
      expect(push).toHaveBeenCalledWith("/add-contact?session=s1");
    });

    it("starts a circle in that session once it has someone to put in it", async () => {
      await actions().find("[data-test='session-new-circle']").trigger("click");
      expect(push).toHaveBeenCalledWith("/new-circle?session=s1");
      store.sessions[0].chats = [];
      expect(actions().find("[data-test='session-new-circle']").exists()).toBe(false);
    });

    it("leaves the session through the core", async () => {
      await actions().find("[data-test='session-close']").trigger("click");
      expect(calls).toContainEqual(["core_session_close", { session: "s1" }]);
    });

    it("deletes the session after asking once", async () => {
      const wrapper = actions();
      await wrapper.find("[data-test='session-remove']").trigger("click");
      expect(calls.some(([command]) => command === "core_session_remove")).toBe(false);
      await wrapper.find("[data-test='session-remove-sure']").trigger("click");
      expect(calls).toContainEqual(["core_session_remove", { session: "s1" }]);
      expect(store.sessions).toHaveLength(0);
    });
  });

  describe("of a session that is only on the screen", () => {
    beforeEach(() => {
      store.sessions = [{ id: "", pin: "135790", chats: [], requests: [], circles: [] }];
    });

    it("looks like any other session, trash included", () => {
      const wrapper = actions();
      expect(wrapper.find("[data-test='session-add']").exists()).toBe(true);
      expect(wrapper.find("[data-test='session-remove']").exists()).toBe(true);
    });

    it("adds nobody, as there is no room", async () => {
      await actions().find("[data-test='session-add']").trigger("click");
      await flushPromises();
      expect(push).not.toHaveBeenCalled();
      expect(calls.some(([command]) => command.startsWith("core_session"))).toBe(false);
    });

    it("goes with the trash without telling the core", async () => {
      const wrapper = actions();
      await wrapper.find("[data-test='session-remove']").trigger("click");
      await wrapper.find("[data-test='session-remove-sure']").trigger("click");
      await flushPromises();
      expect(calls.some(([command]) => command.startsWith("core_session"))).toBe(false);
      expect(store.sessions).toHaveLength(0);
    });
  });
});
