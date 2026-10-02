import { beforeEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonTextarea } from "@ionic/vue";
import FeedbackModal from "./FeedbackModal.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";

/** The router answers every suggestion with `word`, as the core says it. */
function answering(word: string) {
  installTauri((command, args) => {
    calls.push([command, args]);
    return command === "core_send_feedback" ? word : undefined;
  });
}

async function write(wrapper: ReturnType<typeof mount>, text: string) {
  wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", text);
  await flushPromises();
}

const sends = () => calls.filter(([command]) => command === "core_send_feedback");

// 2026-10-02: an anonymous suggestion, mailed on by the router. The modal says "sent" only when the
// router took it, and never loses what was written when it did not. Ioan (2026-10-02): a modal over
// Settings, not a page; it opens and closes like the app's other sheets.
describe("FeedbackModal", () => {
  beforeEach(() => seed());

  it("is a dialog named by its title, with a way out", async () => {
    const wrapper = mount(FeedbackModal, { shallow: true });
    expect(wrapper.find("[role='dialog']").attributes("aria-label")).toBe("Suggest something");
    expect(wrapper.find("h2").text()).toBe("Suggest something");
    const close = wrapper.find("[data-test='feedback-close']");
    expect(close.attributes("aria-label")).toBe("Close");
    await close.trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
  });

  it("goes away when the user taps outside it with nothing written", async () => {
    const wrapper = mount(FeedbackModal, { shallow: true });
    await write(wrapper, "  \n ");
    await wrapper.find("[data-test='feedback-modal']").trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
  });

  // Nothing of a suggestion is kept, so a stray tap outside must not throw away what was written:
  // only the ✕ (and Android's Back) close it then, on purpose.
  it("stays when the user taps outside it with something written, and the ✕ still closes it", async () => {
    const wrapper = mount(FeedbackModal, { shallow: true });
    await write(wrapper, "A long idea I would hate to lose");
    await wrapper.find("[data-test='feedback-modal']").trigger("click");
    expect(wrapper.emitted("close")).toBeUndefined();
    await wrapper.find("[data-test='feedback-close']").trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
  });

  it("stays when the user taps outside it while a suggestion is on its way, and goes once it is sent", async () => {
    let answer: (word: string) => void = () => undefined;
    installTauri((command, args) => {
      calls.push([command, args]);
      return command === "core_send_feedback" ? new Promise((resolve) => (answer = resolve)) : undefined;
    });
    const wrapper = mount(FeedbackModal, { shallow: true });
    await write(wrapper, "Stickers");
    await wrapper.find("[data-test='send']").trigger("click");
    await write(wrapper, "");
    await wrapper.find("[data-test='feedback-modal']").trigger("click");
    expect(wrapper.emitted("close")).toBeUndefined();
    answer("sent");
    await flushPromises();
    await wrapper.find("[data-test='feedback-modal']").trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
  });

  it("always says the suggestion goes without a name and cannot be answered", () => {
    expect(mount(FeedbackModal, { shallow: true }).find("[data-test='hint']").text()).toBe(
      "It reaches us without your name or any identifier. We cannot reply. Do not write personal data.",
    );
  });

  it("cannot send nothing, nor only spaces", async () => {
    const wrapper = mount(FeedbackModal, { shallow: true });
    const button = () => wrapper.find("[data-test='send']");
    expect(button().attributes("disabled")).toBeDefined();
    await write(wrapper, "   \n ");
    expect(button().attributes("disabled")).toBeDefined();
    await write(wrapper, "Stickers");
    expect(button().attributes("disabled")).toBeUndefined();
  });

  it("counts what is written, up to 2000", async () => {
    const wrapper = mount(FeedbackModal, { shallow: true });
    expect(wrapper.find("[data-test='counter']").text()).toBe("0 / 2000");
    await write(wrapper, "Dark mode for the map");
    expect(wrapper.find("[data-test='counter']").text()).toBe("21 / 2000");
    expect(wrapper.findComponent(IonTextarea).attributes("maxlength")).toBe("2000");
  });

  it("sends the text without the spaces around it, thanks and empties the box", async () => {
    answering("sent");
    const wrapper = mount(FeedbackModal, { shallow: true });
    await write(wrapper, "  Stickers, please \n");
    await wrapper.find("[data-test='send']").trigger("click");
    await flushPromises();
    expect(sends()).toEqual([["core_send_feedback", { text: "Stickers, please" }]]);
    expect(wrapper.find("[data-test='outcome']").text()).toBe("Sent. Thank you");
    expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("");
  });

  it("says the day's limit was reached and keeps the text", async () => {
    answering("tooMany");
    const wrapper = mount(FeedbackModal, { shallow: true });
    await write(wrapper, "One more idea");
    await wrapper.find("[data-test='send']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='outcome']").text()).toBe("You have already sent several today. Try again tomorrow");
    expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("One more idea");
  });

  it("says it could not send and keeps the text", async () => {
    answering("failed");
    const wrapper = mount(FeedbackModal, { shallow: true });
    await write(wrapper, "One more idea");
    await wrapper.find("[data-test='send']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='outcome']").text()).toBe("Could not send. Try again later");
    expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("One more idea");
  });

  it("sends once however often it is tapped while sending, with the button off meanwhile", async () => {
    let answer: (word: string) => void = () => undefined;
    installTauri((command, args) => {
      calls.push([command, args]);
      return command === "core_send_feedback" ? new Promise((resolve) => (answer = resolve)) : undefined;
    });
    const wrapper = mount(FeedbackModal, { shallow: true });
    await write(wrapper, "Stickers");
    await wrapper.find("[data-test='send']").trigger("click");
    await wrapper.find("[data-test='send']").trigger("click");
    await flushPromises();
    expect(sends()).toHaveLength(1);
    expect(wrapper.find("[data-test='send']").attributes("disabled")).toBeDefined();
    expect(wrapper.find("[data-test='outcome']").exists()).toBe(false);
    answer("sent");
    await flushPromises();
    expect(wrapper.find("[data-test='outcome']").text()).toBe("Sent. Thank you");
  });
});
