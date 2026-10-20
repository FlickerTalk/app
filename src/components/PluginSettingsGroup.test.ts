import { afterEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonSelect, IonSelectOption, IonToggle } from "@ionic/vue";
import PluginSettingsGroup from "./PluginSettingsGroup.vue";
import { setLocale } from "../i18n";
import type { PluginView } from "../core";
import type { PluginHookResult } from "../plugins";

// 2026-10-20 (plan of the translator §8.2d, F1): a tool's settings for one conversation, drawn by
// the app as a native group from what its settings hook declares: a header with the hook's icon and
// label, a select or a switch per field, and a note. The values and the choices come from the hook
// run out of sight as the group opens; a change runs it again with the values changed, and what it
// answers is shown (a question, a no or a failure leaves the group as it was). Shared by the
// contact's page and a circle's.
const TRANSLATOR: PluginView = {
  id: "com.flickertalk.translate",
  name: "Translator",
  version: "1.1.0",
  kind: "tool",
  asks: { network: [], messages: true, send: "propose", translate: true },
  granted: { network: [], messages: true, send: "propose", translate: true },
  installedAt: 1,
  hooks: {
    settings: {
      label: "Translation",
      icon: "language-outline",
      fields: [
        { key: "lang", type: "select", label: "Language" },
        { key: "receive", type: "toggle", label: "Translate what I receive" },
        { key: "send", type: "toggle", label: "Send translated" },
      ],
      note: "Language packs are downloaded once; translation happens on the phone.",
    },
  },
  locales: { es: { hooks: { settings: { label: "Traducción", fields: { lang: "Idioma", receive: "Traducir lo que recibo" }, note: "Se descargan una vez." } } } },
};
const READ: PluginHookResult = {
  values: { lang: "fr", receive: true, send: false },
  options: { lang: [{ value: "", label: "None" }, { value: "fr", label: "French (detected)" }] },
};

function group(answers: { read?: () => Promise<PluginHookResult | null>; change?: () => Promise<PluginHookResult | null> } = {}) {
  const hooks = {
    readSettings: vi.fn(answers.read ?? (async () => READ)),
    settings: vi.fn(answers.change ?? (async () => null)),
  };
  const wrapper = mount(PluginSettingsGroup, { props: { plugin: TRANSLATOR, hooks }, shallow: true });
  return { wrapper, hooks };
}
const select = (wrapper: ReturnType<typeof mount>) => wrapper.findComponent(IonSelect);
const toggle = (wrapper: ReturnType<typeof mount>, key: string) => wrapper.findAllComponents(IonToggle).find((one) => one.attributes("data-test") === `setting-${key}`)!;
const change = (wrapper: ReturnType<typeof mount>, key: string, checked: boolean) =>
  toggle(wrapper, key).vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked } }));

describe("PluginSettingsGroup", () => {
  afterEach(async () => setLocale("en"));

  it("draws a native group from what the tool declares, at once: its header, a select, switches and its note", () => {
    const { wrapper } = group({ read: () => new Promise(() => {}) });
    expect(wrapper.attributes("data-test")).toBe(`settings-${TRANSLATOR.id}`);
    const header = wrapper.find("ion-list-header-stub");
    expect(header.text()).toBe("Translation");
    expect(header.find("ion-icon-stub").exists()).toBe(true);
    expect(select(wrapper).text()).toContain("Language");
    expect(toggle(wrapper, "receive").text()).toBe("Translate what I receive");
    expect(toggle(wrapper, "send").text()).toBe("Send translated");
    expect(wrapper.find("ion-note-stub").text()).toBe("Language packs are downloaded once; translation happens on the phone.");
    // Nothing to change before the tool has said what is set.
    expect(select(wrapper).attributes("disabled")).toBe("true");
    expect(toggle(wrapper, "receive").attributes("disabled")).toBe("true");
  });

  it("shows the values and the choices the tool says as the group opens", async () => {
    const { wrapper, hooks } = group();
    await flushPromises();
    expect(hooks.readSettings).toHaveBeenCalledWith(TRANSLATOR);
    expect(select(wrapper).attributes("value")).toBe("fr");
    expect(wrapper.findAllComponents(IonSelectOption).map((one) => [one.attributes("value"), one.text()])).toEqual([
      ["", "None"],
      ["fr", "French (detected)"],
    ]);
    expect(toggle(wrapper, "receive").attributes("checked")).toBe("true");
    expect(toggle(wrapper, "send").attributes("checked")).toBe("false");
    expect(toggle(wrapper, "send").attributes("disabled")).toBe("false");
  });

  it("runs the hook with the values changed and shows what it answers", async () => {
    const { wrapper, hooks } = group({ change: async () => ({ values: { lang: "fr", receive: true, send: true }, options: READ.options }) });
    await flushPromises();
    change(wrapper, "send", true);
    await flushPromises();
    expect(hooks.settings).toHaveBeenCalledWith(TRANSLATOR, { manual: true, values: { lang: "fr", receive: true, send: true } });
    expect(toggle(wrapper, "send").attributes("checked")).toBe("true");
    select(wrapper).vm.$emit("ionChange", { detail: { value: "" } });
    await flushPromises();
    expect(hooks.settings).toHaveBeenLastCalledWith(TRANSLATOR, { manual: true, values: { lang: "", receive: true, send: true } });
  });

  it("goes back as it was when the user says no to the tool's question, or the tool fails", async () => {
    const { wrapper } = group({ change: async () => null });
    await flushPromises();
    change(wrapper, "send", true);
    await flushPromises();
    expect(toggle(wrapper, "send").attributes("checked")).toBe("false");
  });

  it("changes nothing while a change is on its way", async () => {
    let release: (result: PluginHookResult | null) => void = () => {};
    const { wrapper, hooks } = group({ change: () => new Promise((resolve) => (release = resolve)) });
    await flushPromises();
    change(wrapper, "send", true);
    await flushPromises();
    expect(toggle(wrapper, "receive").attributes("disabled")).toBe("true");
    change(wrapper, "receive", false);
    expect(hooks.settings).toHaveBeenCalledTimes(1);
    release(READ);
    await flushPromises();
    expect(toggle(wrapper, "receive").attributes("disabled")).toBe("false");
  });

  it("names its settings in the phone's language", async () => {
    await setLocale("es");
    const { wrapper } = group();
    await flushPromises();
    expect(wrapper.find("ion-list-header-stub").text()).toBe("Traducción");
    expect(select(wrapper).text()).toContain("Idioma");
    expect(toggle(wrapper, "receive").text()).toBe("Traducir lo que recibo");
    expect(toggle(wrapper, "send").text(), "in English when the tool does not say").toBe("Send translated");
    expect(wrapper.find("ion-note-stub").text()).toBe("Se descargan una vez.");
  });
});
