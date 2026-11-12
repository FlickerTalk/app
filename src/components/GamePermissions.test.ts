import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { IonButton, IonIcon } from "@ionic/vue";
import { gameControllerOutline, shieldOutline } from "ionicons/icons";
import GamePermissions from "./GamePermissions.vue";
import source from "./GamePermissions.vue?raw";
import { IonModalStub } from "../__tests__/ionic";

const sheet = (props: Record<string, unknown>) =>
  mount(GamePermissions, { props: { open: true, name: "Chess", ...props }, global: { stubs: { IonModal: IonModalStub, IonIcon: true } } });

describe("GamePermissions", () => {
  it("is an Ionic sheet that says what the game will do and lets the user allow it or not", async () => {
    const wrapper = sheet({});
    const modal = wrapper.findComponent(IonModalStub);
    expect(modal.props("isOpen")).toBe(true);
    expect(modal.props("breakpoints")).toContain(modal.props("initialBreakpoint"));
    expect(modal.attributes("aria-label")).toBe("Chess");
    const html = wrapper.find("[data-test='game-permissions']").html();
    expect(html).toContain("Chess");
    expect(html).toContain("This game talks to the other person's phone and can leave the result in the chat.");
    expect(html).not.toContain("🎮");
    expect(wrapper.findComponent(IonIcon).exists()).toBe(true);
    const allow = wrapper.findAllComponents(IonButton).find((one) => one.attributes("data-test") === "game-allow")!;
    expect(allow.element.innerHTML).toContain("Allow and play");

    await wrapper.find("[data-test='game-allow']").trigger("click");
    await wrapper.find("[data-test='game-cancel']").trigger("click");
    expect(wrapper.emitted("allow")).toHaveLength(1);
    expect(wrapper.emitted("cancel")).toHaveLength(1);
  });

  it("says what a game that is not installed weighs, and installs it to play", () => {
    const wrapper = sheet({ size: 1_200_000 });
    expect(wrapper.find("[data-test='game-permissions']").html()).toContain("1.2 MB");
    expect(wrapper.find("[data-test='game-allow']").element.innerHTML).toContain("Install and play");
  });

  it("is a no when Ionic dismisses it", async () => {
    const wrapper = sheet({});
    wrapper.findComponent(IonModalStub).vm.$emit("didDismiss");
    expect(wrapper.emitted("cancel")).toHaveLength(1);
    expect(wrapper.emitted("allow")).toBeUndefined();
  });

  it("shows nothing while closed", () => {
    expect(sheet({ open: false }).find("[data-test='game-permissions']").exists()).toBe(false);
  });

  it("shows the game's own icon, or the controller", () => {
    const icon = (props: Record<string, unknown>) =>
      mount(GamePermissions, { props: { open: true, name: "Chess", ...props }, global: { stubs: { IonModal: IonModalStub } } })
        .find("[data-test='game-permissions']")
        .findComponent(IonIcon)
        .props("icon");
    expect(icon({ icon: shieldOutline })).toBe(shieldOutline);
    expect(icon({})).toBe(gameControllerOutline);
  });

  it("shows the game's own image when it has one", () => {
    const wrapper = mount(GamePermissions, {
      props: { open: true, name: "Chess", image: "data:image/svg+xml;base64,PHN2Zy8+" },
      global: { stubs: { IonModal: IonModalStub } },
    });
    const img = wrapper.find("[data-test='game-permissions'] img");
    expect(img.attributes("src")).toBe("data:image/svg+xml;base64,PHN2Zy8+");
    expect(img.attributes("alt")).toBe("");
    expect(wrapper.find("[data-test='game-permissions']").findComponent(IonIcon).exists()).toBe(false);
  });

  it("leaves the app sheet's room under the handle", () => {
    const styles = source.slice(source.indexOf("<style"));
    expect(styles).toMatch(/\.ft-game-ask__body\s*\{[^}]*padding-top:\s*26px/);
  });

  it("says what it is given instead of the game's text, with its own allow button", () => {
    const wrapper = sheet({ name: "Board", body: "Maria wants to present with Board", allowLabel: "Allow" });
    const html = wrapper.find("[data-test='game-permissions']").html();
    expect(html).toContain("Maria wants to present with Board");
    expect(html).not.toContain("This game talks");
    expect(wrapper.find("[data-test='game-allow']").element.innerHTML).toContain("Allow");
    expect(wrapper.find("[data-test='game-allow']").element.innerHTML).not.toContain("play");
  });
});
