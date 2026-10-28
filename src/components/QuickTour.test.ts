import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { IonButton, IonIcon, IonSegment, IonSegmentButton, IonSegmentContent } from "@ionic/vue";
import {
  appsOutline,
  callOutline,
  chatbubblesOutline,
  flagOutline,
  helpBuoyOutline,
  lockClosedOutline,
  qrCodeOutline,
  swapHorizontalOutline,
} from "ionicons/icons";
import QuickTour from "./QuickTour.vue";
import source from "./QuickTour.vue?raw";
import { IonModalStub } from "../__tests__/ionic";
import { closeAll } from "../back";
import { isTourDue, setTourDue } from "../preferences";
import en from "../i18n/en.json";

// Android's back button: the handler the app listens with while something is open on top.
const back = vi.hoisted(() => ({ handler: null as null | (() => void) }));
vi.mock("@tauri-apps/api/app", () => ({
  onBackButtonPress: async (handler: () => void) => {
    back.handler = handler;
    return { unregister: async () => void (back.handler === handler && (back.handler = null)) };
  },
}));

// Each test's tours go when it ends: one left open would keep Android's back button.
enableAutoUnmount(afterEach);

const tour = (props: Record<string, unknown> = {}) =>
  mount(QuickTour, { props: { open: true, ...props }, global: { stubs: { IonModal: IonModalStub } } });
type Tour = ReturnType<typeof tour>;

const modal = (wrapper: Tour) => wrapper.findComponent(IonModalStub);
const segment = (wrapper: Tour) => wrapper.findComponent(IonSegment);
const button = (wrapper: Tour, test: string) =>
  wrapper.findAllComponents(IonButton).find((one) => one.attributes("data-test") === test)!;

const STEPS = ["contact", "messages", "calls", "apps", "safety", "privacy", "support", "move"];

describe("QuickTour", () => {
  beforeEach(() => {
    localStorage.clear();
    back.handler = null;
  });

  // 2026-10-28 (Ioan: "the typical quick tour"): Ionic's sheet modal, as tall as the screen, that a
  // swipe down closes. It is named for screen readers.
  it("is a full-height Ionic sheet that a swipe down closes", () => {
    const wrapper = tour();
    expect(modal(wrapper).props("isOpen")).toBe(true);
    expect(modal(wrapper).props("breakpoints")).toEqual([0, 1]);
    expect(modal(wrapper).props("initialBreakpoint")).toBe(1);
    expect(modal(wrapper).attributes("aria-label")).toBe("Quick tour");
    expect(modal(wrapper).attributes("data-test")).toBe("quick-tour");
    // The app's other sheets are as tall as what they say; this one keeps Ionic's full height.
    expect(source.slice(source.indexOf("<style"))).not.toMatch(/--height:\s*auto/);
  });

  it("shows nothing while closed", () => {
    expect(tour({ open: false }).find("[data-test='tour-step']").exists()).toBe(false);
  });

  // Eight steps, each one of Ionic's swipeable contents: a large Ionicon, a title and a sentence or
  // two, all from the catalogue. Only what the app really does (brand rules: sober, no "forever").
  it("walks eight steps, each with an Ionicon, a title and what to know", () => {
    const wrapper = tour();
    const steps = wrapper.findAllComponents(IonSegmentContent);
    expect(steps).toHaveLength(8);
    expect(steps.map((step) => step.findComponent(IonIcon).props("icon"))).toEqual([
      qrCodeOutline,
      chatbubblesOutline,
      callOutline,
      appsOutline,
      flagOutline,
      lockClosedOutline,
      helpBuoyOutline,
      swapHorizontalOutline,
    ]);
    expect(steps.map((step) => step.find("h2").text())).toEqual([
      "Add a contact",
      "Write and send",
      "Voice and video calls",
      "Tools and games",
      "Block and report",
      "Privacy",
      "The FlickerTalk contact",
      "A new phone",
    ]);
    expect(steps.map((step) => step.find("p").text())).toEqual([
      "Show your QR code to someone next to you, or share your link. Anyone who writes to you before you add them waits in Requests until you accept them.",
      "Send texts, photos, files and voice messages. To record a voice message, tap the microphone.",
      "In a chat, tap the phone or the camera at the top. Calls ring even when the phone is locked.",
      "The Apps tab has tools and games made and reviewed by FlickerTalk. Tools are free for 15 days, then part of the yearly subscription.",
      "On a contact's page you can block or report them. A report opens an email to info@flickertalk.com with the evidence you choose.",
      "No account and no phone number. Messages are end-to-end encrypted and our server keeps no chats or history: a message that cannot be delivered yet waits there encrypted only if you keep the offline mailbox on.",
      // Installs from before 1.7.1 have no such contact: the step speaks of new installs only.
      "Every new install has a contact called FlickerTalk. It is run by FlickerTalk and answers texts, files, voice messages and calls automatically.",
      "Changing phones? In Settings, tap Move to a new phone and keep both phones close together.",
    ]);
  });

  // Ionic's segment is the row of dots under the steps and says which one is shown; each dot is
  // named for screen readers, as an icon with no words must be.
  it("marks the step shown with one dot per step, each named for screen readers", () => {
    const wrapper = tour();
    expect(segment(wrapper).props("value")).toBe("contact");
    const dots = wrapper.findAllComponents(IonSegmentButton);
    expect(dots.map((dot) => dot.props("value"))).toEqual(STEPS);
    expect(dots.map((dot) => dot.attributes("aria-label"))).toEqual(STEPS.map((_, at) => `Step ${at + 1} of 8`));
    // Each dot leads to its step, so a swipe and a tap on a dot agree.
    const ids = wrapper.findAllComponents(IonSegmentContent).map((step) => step.attributes("id"));
    expect(dots.map((dot) => dot.props("contentId"))).toEqual(ids);
  });

  it("gives its steps ids no other tour on the screen has", () => {
    const one = tour().findAllComponents(IonSegmentContent).map((step) => step.attributes("id"));
    const other = tour().findAllComponents(IonSegmentContent).map((step) => step.attributes("id"));
    expect(new Set([...one, ...other]).size).toBe(16);
  });

  it("goes to the next step with Next, and says Done on the last one", async () => {
    const wrapper = tour();
    expect(button(wrapper, "tour-next").element.innerHTML).toContain("Next");
    for (const step of STEPS.slice(1)) {
      await wrapper.find("[data-test='tour-next']").trigger("click");
      expect(segment(wrapper).props("value")).toBe(step);
    }
    expect(button(wrapper, "tour-next").element.innerHTML).toContain("Done");
    expect(button(wrapper, "tour-next").element.innerHTML).not.toContain("Next");
  });

  // A swipe (or a tap on a dot) moves Ionic's segment, and the button follows it.
  it("follows a swipe to another step", async () => {
    const wrapper = tour();
    segment(wrapper).vm.$emit("ionChange", { detail: { value: "move" } });
    await flushPromises();
    expect(segment(wrapper).props("value")).toBe("move");
    expect(button(wrapper, "tour-next").element.innerHTML).toContain("Done");
    segment(wrapper).vm.$emit("ionChange", { detail: { value: "apps" } });
    await flushPromises();
    expect(button(wrapper, "tour-next").element.innerHTML).toContain("Next");
  });

  // It never stands in the way (App Review): Skip is on every step, and closing is for good.
  it("has Skip on every step, which closes the tour for good", async () => {
    setTourDue();
    const wrapper = tour();
    for (const step of STEPS) {
      segment(wrapper).vm.$emit("ionChange", { detail: { value: step } });
      await flushPromises();
      expect(button(wrapper, "tour-skip").element.innerHTML, step).toContain("Skip");
    }
    await wrapper.find("[data-test='tour-skip']").trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
    expect(isTourDue()).toBe(false);
  });

  it("closes for good with Done", async () => {
    setTourDue();
    const wrapper = tour();
    segment(wrapper).vm.$emit("ionChange", { detail: { value: "move" } });
    await flushPromises();
    await wrapper.find("[data-test='tour-next']").trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
    expect(isTourDue()).toBe(false);
  });

  // A swipe down or a tap outside: Ionic dismisses it on its own, and that is as good as Skip.
  it("closes for good when Ionic dismisses it", () => {
    setTourDue();
    const wrapper = tour();
    modal(wrapper).vm.$emit("didDismiss");
    expect(wrapper.emitted("close")).toHaveLength(1);
    expect(isTourDue()).toBe(false);
  });

  // Its page left the screen (a call, a link that opened the app): the tour goes with it and comes
  // back with it, still owed.
  it("says nothing when closed from outside, and stays owed", async () => {
    setTourDue();
    const wrapper = tour();
    await wrapper.setProps({ open: false });
    modal(wrapper).vm.$emit("didDismiss");
    expect(wrapper.emitted("close")).toBeUndefined();
    expect(isTourDue()).toBe(true);
  });

  // Android's back button closes it as a swipe down would, and lets go of the button once it is shut.
  it("closes for good with the back button", async () => {
    setTourDue();
    const wrapper = tour();
    await flushPromises();
    back.handler?.();
    await flushPromises();
    expect(wrapper.emitted("close")).toHaveLength(1);
    expect(isTourDue()).toBe(false);
    await wrapper.setProps({ open: false });
    await flushPromises();
    expect(back.handler).toBeNull();
  });

  // A link that opened the app closes everything on top to open its page: that is not the user
  // closing the tour, which stays owed and comes back with its page.
  it("is not seen when everything on top closes for a link", async () => {
    setTourDue();
    const wrapper = tour();
    await flushPromises();
    await closeAll();
    expect(wrapper.emitted("close")).toHaveLength(1);
    expect(isTourDue()).toBe(true);
  });

  it("starts again at the first step each time it opens", async () => {
    const wrapper = tour();
    await wrapper.find("[data-test='tour-next']").trigger("click");
    await wrapper.find("[data-test='tour-next']").trigger("click");
    await wrapper.setProps({ open: false });
    await wrapper.setProps({ open: true });
    expect(segment(wrapper).props("value")).toBe("contact");
  });

  // RTL (Arabic): the steps come from the inline end; nothing in the layout is left or right.
  it("lays itself out with logical sides only", () => {
    const styles = source.slice(source.indexOf("<style"));
    expect(styles).not.toMatch(/\b(margin|padding)-(left|right)\b|\btext-align:\s*(left|right)\b|(^|[\s;{])(left|right):/m);
  });
});

// Every one of the 21 catalogues says the whole tour, in the same sober voice as the rest of the app.
describe("the quick tour in every language", () => {
  const catalogues = import.meta.glob<{ tour: Record<string, unknown> }>("../i18n/*.json", { eager: true, import: "default" });
  const KEYS = [
    "title",
    "link",
    "skip",
    "next",
    "done",
    "step",
    ...STEPS.flatMap((step) => [`steps.${step}.title`, `steps.${step}.body`]),
  ];
  const at = (node: unknown, path: string) =>
    path.split(".").reduce<unknown>((into, key) => (into as Record<string, unknown> | undefined)?.[key], node);

  it("ships the tour in 21 catalogues", () => {
    expect(Object.keys(catalogues)).toHaveLength(21);
  });

  it.each(Object.entries(catalogues))("%s has every text of the tour", (_, catalogue) => {
    for (const key of KEYS) {
      const text = at(catalogue.tour, key);
      expect(typeof text, key).toBe("string");
      expect(String(text).trim(), key).not.toBe("");
    }
  });

  it.each(Object.entries(catalogues))("%s says it without exclamations or promises of forever", (_, catalogue) => {
    const said = KEYS.map((key) => String(at(catalogue.tour, key)));
    const forever =
      /\bforever\b|para siempre|para sempre|per sempre|pour toujours|für immer|pentru totdeauna|na zawsze|навсегда|назавжди|sonsuza|selamanya|mãi mãi|ตลอดไป|للأبد|हमेशा के लिए|চিরকাল|ずっと|영원히|永久/i;
    for (const text of said) {
      expect(text).not.toMatch(/[!¡！]/);
      expect(text).not.toMatch(forever);
    }
  });

  it("names the support address only through the app's own constant", () => {
    expect(String(at(en.tour, "steps.safety.body"))).toContain("{address}");
  });
});
