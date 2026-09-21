import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import ChatPage from "./ChatPage.vue";
import ChatThread from "../components/ChatThread.vue";

vi.mock("vue-router", () => ({ useRoute: () => ({ params: { id: "c2" } }) }));

describe("ChatPage", () => {
  it("shows the conversation from the route, with a way back", () => {
    const thread = mount(ChatPage, { shallow: true }).findComponent(ChatThread);
    expect(thread.props("chatId")).toBe("c2");
    expect(thread.props("showBack")).toBe(true);
  });
});
