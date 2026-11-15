import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { IonIcon, IonTitle, IonToolbar } from "@ionic/vue";
import { chatbubblesOutline, closeOutline, contractOutline, mailOutline } from "ionicons/icons";
import GameFullscreenBar from "./GameFullscreenBar.vue";
import source from "./GameFullscreenBar.vue?raw";

const bar = (props: Record<string, unknown> = {}) => mount(GameFullscreenBar, { props: { name: "Chess", ...props }, global: { stubs: { IonIcon: true } } });
const button = (wrapper: ReturnType<typeof bar>, test: string) => wrapper.find(`[data-test='${test}']`);
const icon = (wrapper: ReturnType<typeof bar>, test: string) => button(wrapper, test).findComponent(IonIcon).props("icon");

describe("GameFullscreenBar", () => {
  it("is an Ionic toolbar with the way back first, the game's name and Close last", async () => {
    const wrapper = bar();
    expect(wrapper.findComponent(IonToolbar).exists()).toBe(true);
    expect(wrapper.findComponent(IonTitle).text()).toBe("Chess");
    expect(wrapper.find(".ft-title").attributes("dir")).toBe("auto");

    const exit = button(wrapper, "game-fullscreen-exit");
    expect(exit.attributes("aria-label")).toBe("Exit full screen");
    expect(exit.attributes("aria-pressed")).toBe("true");
    expect(exit.element.closest("ion-buttons")?.getAttribute("slot")).toBe("start");
    expect(icon(wrapper, "game-fullscreen-exit")).toBe(contractOutline);

    const close = button(wrapper, "game-fullscreen-close");
    expect(close.attributes("aria-label")).toBe("Close");
    expect(close.element.closest("ion-buttons")?.getAttribute("slot")).toBe("end");
    expect(icon(wrapper, "game-fullscreen-close")).toBe(closeOutline);

    expect(button(wrapper, "game-fullscreen-chat").exists()).toBe(false);
    expect(button(wrapper, "game-fullscreen-invite").exists()).toBe(false);

    await exit.trigger("click");
    await close.trigger("click");
    expect(wrapper.emitted("exit")).toHaveLength(1);
    expect(wrapper.emitted("close")).toHaveLength(1);
  });

  it("offers the chat after the way back and an invitation before Close when asked to", async () => {
    const wrapper = bar({ chat: true, invite: true });
    const start = wrapper.findAll("ion-buttons[slot='start'] ion-button").map((one) => one.attributes("data-test"));
    const end = wrapper.findAll("ion-buttons[slot='end'] ion-button").map((one) => one.attributes("data-test"));
    expect(start).toEqual(["game-fullscreen-exit", "game-fullscreen-chat"]);
    expect(end).toEqual(["game-fullscreen-invite", "game-fullscreen-close"]);
    expect(button(wrapper, "game-fullscreen-chat").attributes("aria-label")).toBe("Show the chat");
    expect(button(wrapper, "game-fullscreen-invite").attributes("aria-label")).toBe("Invite");
    expect(icon(wrapper, "game-fullscreen-chat")).toBe(chatbubblesOutline);
    expect(icon(wrapper, "game-fullscreen-invite")).toBe(mailOutline);
    expect(wrapper.find("[data-test='game-unread']").exists()).toBe(false);

    await button(wrapper, "game-fullscreen-chat").trigger("click");
    await button(wrapper, "game-fullscreen-invite").trigger("click");
    expect(wrapper.emitted("chat")).toHaveLength(1);
    expect(wrapper.emitted("invite")).toHaveLength(1);
  });

  it("marks the chat with a dot and says so in its name when a message has arrived", () => {
    const wrapper = bar({ chat: true, unread: 2 });
    const dot = wrapper.find("[data-test='game-unread']");
    expect(dot.exists()).toBe(true);
    expect(dot.attributes("aria-hidden")).toBe("true");
    expect(dot.element.closest("[data-test='game-fullscreen-chat']")).not.toBeNull();
    expect(button(wrapper, "game-fullscreen-chat").attributes("aria-label")).toBe("Show the chat, New message");
  });

  it("puts the focus on the way back when asked", () => {
    const wrapper = bar();
    const exit = button(wrapper, "game-fullscreen-exit").element as HTMLElement;
    const focus = vi.spyOn(exit, "focus");
    (wrapper.vm as unknown as { focus: () => void }).focus();
    expect(focus).toHaveBeenCalled();
  });

  it("keeps every button a finger wide", () => {
    const style = source.slice(source.indexOf("<style"));
    expect(style).toMatch(/min-width:\s*44px/);
    expect(style).toMatch(/min-height:\s*44px/);
  });
});
