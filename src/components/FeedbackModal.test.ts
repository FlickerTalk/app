import { beforeEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonModal, IonText, IonTextarea, IonTitle } from "@ionic/vue";
import FeedbackModal from "./FeedbackModal.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";

function answering(word: string) {
  installTauri((command, args) => {
    calls.push([command, args]);
    return command === "core_send_feedback" ? word : undefined;
  });
}

function holding(): { answer: (word: string) => void } {
  const held = { answer: (_word: string) => undefined as void };
  installTauri((command, args) => {
    calls.push([command, args]);
    return command === "core_send_feedback" ? new Promise((resolve) => (held.answer = resolve)) : undefined;
  });
  return held;
}

const open = () => mount(FeedbackModal, { props: { open: true }, shallow: true });

async function write(wrapper: ReturnType<typeof mount>, text: string) {
  wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", text);
  await flushPromises();
}

async function send(wrapper: ReturnType<typeof mount>) {
  await wrapper.find("[data-test='send']").trigger("click");
  await flushPromises();
}

async function mayDismiss(wrapper: ReturnType<typeof mount>, role?: string): Promise<boolean> {
  const canDismiss = wrapper.findComponent(IonModal).props("canDismiss") as (data?: unknown, role?: string) => Promise<boolean>;
  return canDismiss(undefined, role);
}

const sends = () => calls.filter(([command]) => command === "core_send_feedback");
const outcome = (wrapper: ReturnType<typeof mount>) => wrapper.find("[data-test='outcome']");

describe("FeedbackModal", () => {
  beforeEach(() => seed());

  it("is an Ionic modal, open as Settings says, titled, with a way out", async () => {
    const wrapper = open();
    expect(wrapper.findComponent(IonModal).props("isOpen")).toBe(true);
    expect(mount(FeedbackModal, { props: { open: false }, shallow: true }).findComponent(IonModal).props("isOpen")).toBe(false);
    expect(wrapper.findComponent(IonTitle).text()).toBe("Suggest something");
    const close = wrapper.find("[data-test='feedback-close']");
    expect(close.attributes("aria-label")).toBe("Close");
    await close.trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
  });

  it("always says the suggestion goes without a name and cannot be answered", () => {
    expect(open().find("[data-test='hint']").text()).toBe(
      "It reaches us without your name or any identifier. We cannot reply. Do not write personal data.",
    );
  });

  it("counts what is written with Ionic's counter, up to 2000", () => {
    const box = open().findComponent(IonTextarea);
    expect(box.props("counter")).toBe(true);
    expect(box.props("maxlength")).toBe(2000);
  });

  it("cannot send nothing, nor only spaces", async () => {
    const wrapper = open();
    const button = () => wrapper.find("[data-test='send']");
    expect(button().attributes("disabled")).toBe("true");
    await write(wrapper, "   \n ");
    expect(button().attributes("disabled")).toBe("true");
    await write(wrapper, "Stickers");
    expect(button().attributes("disabled")).toBe("false");
  });

  it("sends the text without the spaces around it, thanks and empties the box", async () => {
    answering("sent");
    const wrapper = open();
    await write(wrapper, "  Stickers, please \n");
    await send(wrapper);
    expect(sends()).toEqual([["core_send_feedback", { text: "Stickers, please" }]]);
    expect(outcome(wrapper).text()).toBe("Sent. Thank you");
    expect(wrapper.findComponent(IonText).exists()).toBe(true);
    expect(outcome(wrapper).attributes("color")).toBe("success");
    expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("");
  });

  it("says the day's limit was reached, in Ionic's danger colour, and keeps the text", async () => {
    answering("tooMany");
    const wrapper = open();
    await write(wrapper, "One more idea");
    await send(wrapper);
    expect(outcome(wrapper).text()).toBe("You have already sent several today. Try again tomorrow");
    expect(outcome(wrapper).attributes("color")).toBe("danger");
    expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("One more idea");
  });

  it("says it could not send, in Ionic's danger colour, and keeps the text", async () => {
    answering("failed");
    const wrapper = open();
    await write(wrapper, "One more idea");
    await send(wrapper);
    expect(outcome(wrapper).text()).toBe("Could not send. Try again later");
    expect(outcome(wrapper).attributes("color")).toBe("danger");
    expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("One more idea");
  });

  it("sends once however often it is tapped while sending, with the button off meanwhile", async () => {
    const held = holding();
    const wrapper = open();
    await write(wrapper, "Stickers");
    await wrapper.find("[data-test='send']").trigger("click");
    await wrapper.find("[data-test='send']").trigger("click");
    await flushPromises();
    expect(sends()).toHaveLength(1);
    expect(wrapper.find("[data-test='send']").attributes("disabled")).toBe("true");
    expect(outcome(wrapper).exists()).toBe(false);
    held.answer("sent");
    await flushPromises();
    expect(outcome(wrapper).text()).toBe("Sent. Thank you");
  });

  it("shows the thanks until something new is typed, then lets it go", async () => {
    answering("sent");
    const wrapper = open();
    await write(wrapper, "Stickers, please");
    await send(wrapper);
    expect(outcome(wrapper).text()).toBe("Sent. Thank you");
    await flushPromises();
    expect(outcome(wrapper).exists()).toBe(true);
    await write(wrapper, "A");
    expect(outcome(wrapper).exists()).toBe(false);
  });

  it("shows why it did not go until the kept text is edited, then lets it go", async () => {
    for (const word of ["failed", "tooMany"]) {
      answering(word);
      const wrapper = open();
      await write(wrapper, "One more idea");
      await send(wrapper);
      expect(outcome(wrapper).exists()).toBe(true);
      await write(wrapper, "One more idea!");
      expect(outcome(wrapper).exists(), word).toBe(false);
      expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("One more idea!");
    }
  });

  it("lets a tap outside dismiss it only with nothing written", async () => {
    const wrapper = open();
    expect(await mayDismiss(wrapper, "backdrop")).toBe(true);
    await write(wrapper, "  \n ");
    expect(await mayDismiss(wrapper, "backdrop")).toBe(true);
    await write(wrapper, "A long idea I would hate to lose");
    expect(await mayDismiss(wrapper, "backdrop")).toBe(false);
    expect(await mayDismiss(wrapper, "gesture")).toBe(false);
  });

  it("lets a tap outside dismiss it neither while a suggestion is on its way, nor until it is sent", async () => {
    const held = holding();
    const wrapper = open();
    await write(wrapper, "Stickers");
    await wrapper.find("[data-test='send']").trigger("click");
    await write(wrapper, "");
    expect(await mayDismiss(wrapper, "backdrop")).toBe(false);
    held.answer("sent");
    await flushPromises();
    expect(await mayDismiss(wrapper, "backdrop")).toBe(true);
  });

  it("always lets the ✕ and Back dismiss it, whatever is written", async () => {
    const wrapper = open();
    await write(wrapper, "A long idea I would hate to lose");
    expect(await mayDismiss(wrapper, undefined)).toBe(true);
  });

  it("forgets the text and the outcome once dismissed, and says it closed", async () => {
    answering("failed");
    const wrapper = open();
    await write(wrapper, "One more idea");
    await send(wrapper);
    wrapper.findComponent(IonModal).vm.$emit("didDismiss", new CustomEvent("didDismiss"));
    await flushPromises();
    expect(wrapper.emitted("close")).toHaveLength(1);
    expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("");
    expect(outcome(wrapper).exists()).toBe(false);
  });
});
