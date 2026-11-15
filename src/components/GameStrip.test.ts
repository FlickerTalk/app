import { afterEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { IonIcon } from "@ionic/vue";
import { chatbubblesOutline, chevronDownOutline, chevronUpOutline, gameControllerOutline } from "ionicons/icons";
import GameStrip from "./GameStrip.vue";

const strip = (props: Record<string, unknown> = {}) => mount(GameStrip, { props: { peeking: false, name: "Chess", ...props }, global: { stubs: { IonIcon: true } } });
const icons = (wrapper: ReturnType<typeof strip>) => wrapper.findAllComponents(IonIcon).map((one) => one.props("icon"));

describe("GameStrip", () => {
  afterEach(() => vi.restoreAllMocks());

  it("offers the chat under the game, with the other one's last line when there is one", async () => {
    const wrapper = strip();
    expect(wrapper.find("ion-label").element.innerHTML).toBe("Show the chat");
    expect(wrapper.find("[data-test='game-peek']").attributes("aria-label")).toBe("Show the chat");
    expect(icons(wrapper)).toEqual([chatbubblesOutline, chevronUpOutline]);

    await wrapper.setProps({ last: { id: "m1", mine: false, text: "¿otra?", time: "10:00", sentAt: 0 } });
    expect(wrapper.find("ion-label").element.innerHTML).toBe("¿otra?");
    expect(wrapper.find("ion-label").attributes("dir")).toBe("auto");
    await wrapper.setProps({ last: { id: "m2", mine: false, text: "", kind: "file", file: { name: "board.png", size: "1 KB", mime: "image/png", progress: 1, state: "done" }, time: "10:01", sentAt: 0 } });
    expect(wrapper.find("ion-label").element.innerHTML).toBe("board.png");
  });

  it("offers the game back with its name while the chat is shown", () => {
    const wrapper = strip({ peeking: true });
    expect(wrapper.find("ion-label").element.innerHTML).toBe("Chess");
    expect(wrapper.find("[data-test='game-peek']").attributes("aria-label")).toBe("Back to the game");
    expect(icons(wrapper)).toEqual([gameControllerOutline, chevronDownOutline]);
  });

  it("asks once to switch from the strip or from its button", async () => {
    const wrapper = strip();
    await wrapper.find("[data-test='game-strip']").trigger("click");
    await wrapper.find("[data-test='game-peek']").trigger("click");
    expect(wrapper.emitted("peek")).toHaveLength(2);
  });

  it("puts the focus on its button when asked", () => {
    const wrapper = strip({ peeking: true });
    const focus = vi.spyOn(wrapper.find("[data-test='game-peek']").element as HTMLElement, "focus");
    (wrapper.vm as unknown as { focus: () => void }).focus();
    expect(focus).toHaveBeenCalled();
  });
});
