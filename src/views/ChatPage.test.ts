import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import ChatPage from "./ChatPage.vue";
import ChatThread from "../components/ChatThread.vue";

const route = vi.hoisted(() => ({ params: { id: "c2" }, query: {} as Record<string, unknown> }));
vi.mock("vue-router", () => ({ useRoute: () => route }));

describe("ChatPage", () => {
  it("shows the conversation from the route, with a way back", () => {
    const thread = mount(ChatPage, { shallow: true }).findComponent(ChatThread);
    expect(thread.props("chatId")).toBe("c2");
    expect(thread.props("showBack")).toBe(true);
    expect(thread.props("play")).toBeUndefined();
  });

  // Plan 10.4: the games tab opens a game in the conversation with the contact picked.
  it("hands the conversation the game the address asks to play", () => {
    route.query = { play: "com.flickertalk.game.chess" };
    const thread = mount(ChatPage, { shallow: true }).findComponent(ChatThread);
    expect(thread.props("play")).toBe("com.flickertalk.game.chess");
  });
});
