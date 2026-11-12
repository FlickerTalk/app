import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { IonButton, IonFabList, IonTextarea } from "@ionic/vue";
import type { ChatMessage } from "../core";
import ChatComposer from "./ChatComposer.vue";
import EmojiPicker from "./EmojiPicker.vue";

const THEIRS: ChatMessage = { id: "m1", mine: false, text: "Are you coming tonight?", time: "09:00", sentAt: 1 };
const MINE: ChatMessage = { id: "m2", mine: true, text: "Yes! Leaving work at 5:30", time: "09:01", sentAt: 1 };

const composer = (props: Record<string, unknown> = {}) =>
  mount(ChatComposer, {
    props: {
      draft: "",
      emoji: false,
      attaching: false,
      error: "",
      staged: null,
      editing: null,
      replying: null,
      contactName: "Maria López",
      playing: false,
      recording: false,
      elapsed: "0:00",
      ...props,
    },
    global: { stubs: { IonIcon: true } },
  });

const button = (wrapper: ReturnType<typeof composer>, label: string) => wrapper.find(`[aria-label='${label}']`);

describe("ChatComposer", () => {
  it("offers the microphone while there is nothing to send, and send and later once there are words", async () => {
    const empty = composer();
    expect(button(empty, "Record voice message").exists()).toBe(true);
    expect(button(empty, "Send").exists()).toBe(false);
    expect(empty.find("[data-test='send-later']").exists()).toBe(false);
    await button(empty, "Record voice message").trigger("click");
    expect(empty.emitted("record")).toHaveLength(1);

    const written = composer({ draft: "hi" });
    expect(button(written, "Record voice message").exists()).toBe(false);
    await button(written, "Send").trigger("click");
    await written.find("[data-test='send-later']").trigger("click");
    expect(written.emitted("send")).toHaveLength(1);
    expect(written.emitted("sendLater")).toHaveLength(1);
    expect(composer({ draft: "fixed", editing: MINE }).find("[data-test='send-later']").exists()).toBe(false);
  });

  it("hands up what is typed and the emoji picked", async () => {
    const wrapper = composer({ draft: "see you", emoji: true });
    wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "see you there");
    expect(wrapper.emitted("update:draft")).toEqual([["see you there"]]);
    wrapper.findComponent(EmojiPicker).vm.$emit("pick", "🎉");
    expect(wrapper.emitted("addEmoji")).toEqual([["🎉"]]);
    await wrapper.find("[data-test='open-emoji']").trigger("click");
    expect(wrapper.emitted("update:emoji")).toEqual([[false]]);
    expect(composer().findComponent(EmojiPicker).exists()).toBe(false);
  });

  it("shows the recording with its time instead of the words, to send or to throw away", async () => {
    const wrapper = composer({ recording: true, elapsed: "1:05" });
    expect(wrapper.find("[data-test='recording']").text()).toContain("1:05");
    expect(wrapper.findComponent(IonTextarea).exists()).toBe(false);
    expect(wrapper.find("[data-test='open-emoji']").exists()).toBe(false);
    await button(wrapper, "Discard voice message").trigger("click");
    await button(wrapper, "Send voice message").trigger("click");
    expect(wrapper.emitted("discardVoice")).toHaveLength(1);
    expect(wrapper.emitted("sendVoice")).toHaveLength(1);
  });

  it("unfolds photo or video, camera and file from the attach button, and hides it during a game", async () => {
    const wrapper = composer({ attaching: true });
    expect(wrapper.findComponent(IonFabList).props("activated")).toBe(true);
    await button(wrapper, "Attach").trigger("click");
    expect(wrapper.emitted("update:attaching")).toEqual([[false]]);
    await button(wrapper, "Attach photo or video").trigger("click");
    await button(wrapper, "Take a photo").trigger("click");
    await button(wrapper, "Attach file").trigger("click");
    expect(wrapper.emitted("pick")).toEqual([["image/*,video/*"], [""]]);
    expect(wrapper.emitted("snap")).toHaveLength(1);
    await wrapper.find("input[type='file']").trigger("change");
    expect(wrapper.emitted("attach")).toHaveLength(1);
    expect((wrapper.vm as unknown as { picker: HTMLInputElement }).picker.type).toBe("file");

    const playing = composer({ playing: true });
    expect(button(playing, "Attach").exists()).toBe(false);
    expect(playing.find("[data-test='open-emoji']").exists()).toBe(true);
  });

  it("says what went wrong above the composer", () => {
    expect(composer({ error: "Can't take a photo" }).find("[role='alert']").text()).toBe("Can't take a photo");
    expect(composer().find("[role='alert']").exists()).toBe(false);
  });

  it("shows a staged file to send or throw away", async () => {
    const wrapper = composer({ staged: { path: "/p/clean.jpg", name: "clean.jpg", mime: "image/jpeg", size: 3 } });
    expect(wrapper.find("[data-test='staged']").text()).toContain("clean.jpg");
    await wrapper.find("[data-test='staged-discard']").trigger("click");
    await wrapper.find("[data-test='staged-send']").trigger("click");
    expect(wrapper.emitted("discardStaged")).toHaveLength(1);
    expect(wrapper.emitted("sendStaged")).toHaveLength(1);
  });

  it("quotes what is being answered or edited, with Ionic's clear buttons to drop it", async () => {
    const answering = composer({ replying: THEIRS });
    expect(answering.find("[data-test='replying']").text()).toContain("Maria López");
    expect(answering.find("[data-test='replying'] .ft-replying__quote").text()).toBe("Are you coming tonight?");
    const cancel = answering.findAllComponents(IonButton).find((one) => one.attributes("data-test") === "cancel-reply")!;
    expect(cancel.props("fill")).toBe("clear");
    await cancel.trigger("click");
    expect(answering.emitted("cancelReply")).toHaveLength(1);
    expect(composer({ replying: MINE }).find("[data-test='replying']").text()).toContain("You");
    expect(
      composer({ replying: { ...MINE, kind: "file", text: "", file: { name: "menu.pdf" } as ChatMessage["file"] } })
        .find(".ft-replying__quote")
        .text(),
    ).toBe("menu.pdf");

    const editing = composer({ editing: MINE });
    expect(editing.find("[data-test='editing']").text()).toContain("Yes! Leaving work at 5:30");
    await editing.find("[data-test='cancel-edit']").trigger("click");
    expect(editing.emitted("cancelEdit")).toHaveLength(1);
  });
});
