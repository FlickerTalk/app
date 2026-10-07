import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonBackButton, IonIcon, IonList, IonNote, IonToggle } from "@ionic/vue";
import { addCircleOutline, downloadOutline, lockClosedOutline } from "ionicons/icons";
import PluginsPage from "./PluginsPage.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import { setLocale } from "../i18n";
import { installed, toolsLocked } from "../plugins";

const route = vi.hoisted(() => ({ path: "/plugins" }));
const nav = vi.hoisted(() => ({ push: vi.fn() }));
vi.mock("vue-router", () => ({ useRoute: () => route, useRouter: () => nav }));

const CODE = {
  id: "com.flickertalk.code",
  name: "Code block",
  version: "1.0.0",
  asks: { network: [], messages: true, send: "nothing" },
  granted: { network: [], messages: false, send: "nothing" },
  installedAt: 1_800_000_000_000,
};
const AI = {
  id: "com.flickertalk.ai",
  name: "Assistant",
  version: "0.2.0",
  asks: { network: ["api.openai.com"], messages: true, send: "propose" },
  granted: { network: [], messages: true, send: "nothing" },
  installedAt: 1_800_000_000_000,
};

/** What the catalogue offers this phone; nothing travels inside the app (§56). */
/** The switches of one plugin's group: the list is sorted by name, not in the core's order. */
const togglesOf = (wrapper: ReturnType<typeof mount>, id: string) =>
  wrapper.findAllComponents(IonList).find((list) => list.find(`[data-test='open-${id}']`).exists())!.findAllComponents(IonToggle);

const OFFERED = [
  { id: "com.flickertalk.code", name: "Code block", version: "1.0.0", summary: "Shows code.", size: 2048, installed: true, carried: true },
  { id: "com.flickertalk.sketch", name: "Sketch", version: "1.0.0", summary: "Draw with a finger.", size: 3072, installed: false, carried: true },
  { id: "com.flickertalk.ocr", name: "Read text", version: "1.0.0", summary: "Reads the text of a picture.", size: 5_400_000, installed: false, carried: false },
];

describe("PluginsPage", () => {
  beforeEach(() => {
    seed();
    // The seeded bridge is replaced, so the calls are recorded here.
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_plugins") return [CODE, AI];
      if (command === "core_catalogue") return OFFERED;
      return undefined;
    });
  });

  // 2026-10-02 (plan of the catalogue's translations): a Spanish phone names each tool in Spanish:
  // from its package, or from the catalogue when the package installed here has no translation.
  describe("on a Spanish phone", () => {
    afterEach(() => setLocale("en"));

    it("names installed and offered tools in Spanish, with their summary", async () => {
      await setLocale("es");
      installTauri((command) => {
        if (command === "core_plugins") return [{ ...CODE, locales: { es: { name: "Bloque de código" } } }, AI];
        if (command === "core_catalogue") {
          return [
            OFFERED[0],
            { ...OFFERED[1], locales: { es: { name: "Dibujo", summary: "Dibuja con el dedo." } } },
            OFFERED[2],
            { id: AI.id, name: "Assistant", version: "0.2.0", summary: "Answers.", size: 1, installed: true, carried: false, locales: { es: { name: "Asistente" } } },
          ];
        }
        return undefined;
      });
      const wrapper = mount(PluginsPage, { shallow: true });
      await flushPromises();
      const text = wrapper.text();
      expect(text).toContain("Bloque de código");
      expect(text).toContain("Asistente");
      expect(text).not.toContain("Assistant");
      expect(text).toContain("Dibujo");
      expect(text).toContain("Dibuja con el dedo.");
      // Sorted as the phone reads them: "Asistente" before "Bloque de código".
      expect(text.indexOf("Asistente")).toBeLessThan(text.indexOf("Bloque de código"));
    });
  });

  // 2026-10-03 (updates): an update the core made in the background shows at once.
  it("shows a plugin updated in the background without being opened again", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).not.toContain("1.0.1");
    installed.value = [{ ...CODE, version: "1.0.1" }, AI] as never;
    await flushPromises();
    expect(wrapper.text()).toContain("1.0.1");
  });

  // 2026-10-05: the same screen is a tab when Settings puts the plugins on the bar; there it has
  // nowhere to go back to.
  it("goes back to Settings when opened from there, and has no back button as a tab", async () => {
    route.path = "/plugins";
    expect(mount(PluginsPage, { shallow: true }).findComponent(IonBackButton).exists()).toBe(true);
    route.path = "/tabs/plugins";
    expect(mount(PluginsPage, { shallow: true }).findComponent(IonBackButton).exists()).toBe(false);
    route.path = "/plugins";
  });

  it("lists the plugins on this phone with their version", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("Code block");
    expect(wrapper.text()).toContain("1.0.0");
    expect(wrapper.text()).toContain("Assistant");
  });

  // §53: what a plugin may do is shown one by one, and nothing is on until the user says so.
  it("shows every permission a plugin asks for, and whether it is on", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    const text = wrapper.text();
    expect(text).toContain("Read what you send it");
    expect(text).toContain("api.openai.com");
    expect(text).toContain("Write in the chat");
    expect(wrapper.findAllComponents(IonToggle).length).toBeGreaterThanOrEqual(4);
    expect(togglesOf(wrapper, "com.flickertalk.code")[0].props("checked")).toBe(false);
  });

  it("grants a permission through the core", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    const toggle = togglesOf(wrapper, "com.flickertalk.code")[0];
    toggle.vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: true } }));
    await flushPromises();
    expect(calls).toContainEqual([
      "core_plugin_grant",
      { plugin: "com.flickertalk.code", granted: { network: [], messages: true, send: "nothing" } },
    ]);
  });

  // QA of 1.4.0 (2026-10-06): two switches of one plugin turned on quickly lost the first; the
  // second grant was built from the plugin as it was before either. Each change starts from what
  // the core says now, and the switches end showing what the core has.
  describe("switches turned quickly, one after another", () => {
    const TWO = {
      id: "com.flickertalk.two",
      name: "Two",
      version: "1.0.0",
      asks: { network: ["api.example.com"], messages: true, send: "nothing" },
      granted: { network: [] as string[], messages: false, send: "nothing" },
      installedAt: 1,
    };
    let granted = { ...TWO.granted };
    let refuses = false;
    const flip = (toggle: ReturnType<typeof togglesOf>[number], checked: boolean, target?: { checked: boolean }) => {
      const event = new CustomEvent("ionChange", { detail: { checked } });
      if (target) Object.defineProperty(event, "target", { value: target });
      toggle.vm.$emit("ionChange", event);
    };

    beforeEach(() => {
      granted = { ...TWO.granted, network: [] };
      refuses = false;
      installTauri(async (command, args) => {
        calls.push([command, args]);
        if (command === "core_plugins") return [{ ...TWO, granted }];
        if (command === "core_plugin_grant") {
          await new Promise((done) => setTimeout(done, 5));
          if (refuses) throw new Error("refused");
          granted = (args as { granted: typeof granted }).granted;
          return undefined;
        }
        return command === "core_catalogue" ? [] : undefined;
      });
    });

    it("keeps both", async () => {
      const wrapper = mount(PluginsPage, { shallow: true });
      await flushPromises();
      const [first, second] = togglesOf(wrapper, TWO.id);
      flip(first, true);
      flip(second, true);
      await vi.waitFor(() => expect(calls.filter(([command]) => command === "core_plugin_grant")).toHaveLength(2));
      await vi.waitFor(() => expect(granted).toEqual({ network: ["api.example.com"], messages: true, send: "nothing" }));
      await flushPromises();
      expect(togglesOf(wrapper, TWO.id).map((toggle) => toggle.props("checked"))).toEqual([true, true]);
    });

    it("puts a switch back when the core does not take the change", async () => {
      const wrapper = mount(PluginsPage, { shallow: true });
      await flushPromises();
      refuses = true;
      const field = { checked: true };
      flip(togglesOf(wrapper, TWO.id)[0], true, field);
      await vi.waitFor(() => expect(field.checked).toBe(false));
      expect(granted.messages).toBe(false);
    });
  });

  it("removes a plugin after asking", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='remove-com.flickertalk.code']").trigger("click");
    expect(calls.map(([command]) => command)).not.toContain("core_plugin_remove");
    await wrapper.find("[data-test='remove-confirm']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_plugin_remove", { plugin: "com.flickertalk.code" }]);
  });

  // A tool that travels with the app is offered, never installed behind the user's back (§53).
  it("offers the tools the app carries that are not installed yet", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("Sketch");
    expect(wrapper.text()).toContain("Draw with a finger.");
    expect(wrapper.find("[data-test='install-com.flickertalk.sketch']").exists()).toBe(true);
    expect(wrapper.find("[data-test='install-com.flickertalk.code']").exists()).toBe(false);
  });

  // 2026-10-03: adding a tool the app carries downloads nothing, so it is not shown as a download.
  it("shows a carried tool as an addition and a catalogue one as a download", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    const icon = (id: string) => wrapper.find(`[data-test='install-${id}']`).findComponent(IonIcon).props("icon");
    expect(icon("com.flickertalk.sketch")).toBe(addCircleOutline);
    expect(icon("com.flickertalk.ocr")).toBe(downloadOutline);
  });

  it("installs one when the user asks for it", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='install-com.flickertalk.sketch']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_plugin_add", { plugin: "com.flickertalk.sketch" }]);
    expect(calls.filter(([command]) => command === "core_plugins").length).toBeGreaterThan(1);
  });

  // app#75: offline, the install was rejected and the screen showed nothing; the tap seemed lost.
  it("says so when a tool cannot be installed", async () => {
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_plugins") return [CODE, AI];
      if (command === "core_catalogue") return OFFERED;
      if (command === "core_plugin_add") throw new Error("download failed");
      return undefined;
    });
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='install-com.flickertalk.ocr']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[role='alert']").text()).toBe("The tool could not be installed. Check your connection and try again.");
    // An Ionic note in the palette's danger colour, not a hand-made paragraph.
    const note = wrapper.findAllComponents(IonNote).find((one) => one.attributes("role") === "alert");
    expect(note?.props("color")).toBe("danger");
    // The tool is still offered, so the user can try again once the network is back.
    expect(wrapper.find("[data-test='install-com.flickertalk.ocr']").attributes("disabled")).toBeUndefined();
  });

  it("shows that it is installing until the core answers", async () => {
    let finish = () => {};
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_plugins") return [CODE, AI];
      if (command === "core_catalogue") return OFFERED;
      if (command === "core_plugin_add") return new Promise<void>((resolve) => (finish = resolve));
      return undefined;
    });
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    const button = () => wrapper.find("[data-test='install-com.flickertalk.ocr']");
    await button().trigger("click");
    await flushPromises();
    expect(button().attributes("disabled")).toBeDefined();
    expect(button().attributes("aria-busy")).toBe("true");
    finish();
    await flushPromises();
    expect(button().attributes("aria-busy")).toBeUndefined();
  });

  // What is downloaded says what it will cost; what the app already carries costs nothing and
  // says nothing (§52).
  it("says what a tool weighs before it is downloaded", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    const text = wrapper.text();
    expect(text).toContain("5.4 MB");
    expect(wrapper.find("[data-test='install-com.flickertalk.sketch']").exists()).toBe(true);
    expect(text).not.toContain("3 KB");
  });

  // Plan 10.3: games have their own section; Settings shows tools only, installed or offered.
  it("shows tools only, never a game", async () => {
    const chess = { ...CODE, id: "com.flickertalk.game.chess", name: "Chess", kind: "game" };
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_plugins") return [CODE, chess];
      if (command === "core_catalogue") {
        return [...OFFERED, { id: "com.flickertalk.game.go", name: "Go", version: "1.0.0", summary: "Play go.", size: 9000, installed: false, carried: false, kind: "game" }];
      }
      return undefined;
    });
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("Code block");
    expect(wrapper.text()).toContain("Sketch");
    expect(wrapper.text()).not.toContain("Chess");
    expect(wrapper.find("[data-test='install-com.flickertalk.game.go']").exists()).toBe(false);
  });

  // 2026-09-27: the live channel, reminders, the cloud and the room are switches of their own.
  it("shows a switch for each of the new permissions a plugin asks for", async () => {
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_plugins") {
        return [
          {
            id: "com.flickertalk.board",
            name: "Board",
            version: "1.0.0",
            asks: { network: [], messages: false, send: "propose", live: true, remind: true, drive: true, storage: "large" },
            granted: { network: [], messages: false, send: "nothing", live: false, remind: false, drive: false, storage: "small" },
            installedAt: 1,
            opens: ["application/x-ftboard"],
          },
        ];
      }
      return command === "core_catalogue" ? [] : undefined;
    });
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    const text = wrapper.text();
    for (const label of [
      "Talk to the same plugin on the other side of the chat",
      "Set reminders on this phone",
      "Keep files in your own cloud",
      "Keep a lot of data on this phone (up to 256 MB)",
    ]) {
      expect(text).toContain(label);
    }
    // Writing, the channel, reminders, the cloud and the room: the room is the last switch.
    const toggles = wrapper.findAllComponents(IonToggle);
    expect(toggles).toHaveLength(5);
    toggles[4].vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: true } }));
    await flushPromises();
    const granted = calls.find(([command]) => command === "core_plugin_grant");
    expect(granted?.[1]).toMatchObject({ plugin: "com.flickertalk.board", granted: { storage: "large", live: false } });
  });

  // 2026-10-02: the location plugin asks for the phone's position on a switch of its own, off
  // until the user turns it on; turning it on grants just that.
  it("shows a switch for the phone's position and grants it when turned on", async () => {
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_plugins") {
        return [
          {
            id: "com.flickertalk.location",
            name: "Location",
            version: "1.0.0",
            asks: { network: [], messages: false, send: "propose", location: true },
            granted: { network: [], messages: false, send: "propose", location: false },
            installedAt: 1,
          },
        ];
      }
      return command === "core_catalogue" ? [] : undefined;
    });
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("Your location, only when you ask");
    // Writing, then the position.
    const toggles = wrapper.findAllComponents(IonToggle);
    expect(toggles).toHaveLength(2);
    toggles[1].vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: true } }));
    await flushPromises();
    const granted = calls.find(([command]) => command === "core_plugin_grant");
    expect(granted?.[1]).toMatchObject({ plugin: "com.flickertalk.location", granted: { location: true, send: "propose" } });
  });

  // Ioan, 2026-10-08: after the free year, without the subscription, the tools are locked. Each
  // shows a lock, and a tap on it goes to the Plan screen, where the subscription is.
  describe("once the free year is over", () => {
    function limited(state = "limited") {
      nav.push.mockReset();
      installTauri((command, args) => {
        calls.push([command, args]);
        if (command === "core_plugins") return [CODE, AI];
        if (command === "core_catalogue") return OFFERED;
        if (command === "core_plan") return { state, until: 0 };
        return undefined;
      });
    }
    afterEach(() => (toolsLocked.value = false));

    it("locks the installed tools and opens the Plan screen instead", async () => {
      limited();
      const wrapper = mount(PluginsPage, { shallow: true });
      await flushPromises();
      const open = wrapper.find(`[data-test='open-${CODE.id}']`);
      expect(open.findComponent(IonIcon).props("icon")).toBe(lockClosedOutline);
      expect(open.attributes("aria-label")).toBe("Subscribe to use the tools");
      await open.trigger("click");
      expect(nav.push).toHaveBeenCalledWith("/plan");
      expect(nav.push).not.toHaveBeenCalledWith(`/plugin/${CODE.id}`);
    });

    it("locks what could be installed, and installs nothing", async () => {
      limited();
      const wrapper = mount(PluginsPage, { shallow: true });
      await flushPromises();
      const install = wrapper.find("[data-test='install-com.flickertalk.sketch']");
      expect(install.findComponent(IonIcon).props("icon")).toBe(lockClosedOutline);
      await install.trigger("click");
      await flushPromises();
      expect(nav.push).toHaveBeenCalledWith("/plan");
      expect(calls.map(([command]) => command)).not.toContain("core_plugin_add");
    });

    it("says why, with the way to subscribe", async () => {
      limited();
      const wrapper = mount(PluginsPage, { shallow: true });
      await flushPromises();
      expect(wrapper.find("[data-test='locked']").text()).toContain("tools need the subscription");
      await wrapper.find("[data-test='subscribe']").trigger("click");
      expect(nav.push).toHaveBeenCalledWith("/plan");
    });

    it("locks nothing in the free year or with the subscription", async () => {
      for (const state of ["trial", "subscribed"]) {
        limited(state);
        const wrapper = mount(PluginsPage, { shallow: true });
        await flushPromises();
        expect(wrapper.find("[data-test='locked']").exists(), state).toBe(false);
        await wrapper.find(`[data-test='open-${CODE.id}']`).trigger("click");
        expect(nav.push, state).toHaveBeenCalledWith(`/plugin/${CODE.id}`);
      }
    });

    // The plan may close while the page is open (the year ends): the core refuses, and the user
    // is taken where the subscription is rather than told the install failed.
    it("goes to the Plan screen when the core refuses a tool", async () => {
      limited("trial");
      const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, args?: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__;
      const answer = internals.invoke;
      internals.invoke = (command, args) => (command === "core_plugin_add" ? Promise.reject("needs_subscription") : answer(command, args));
      const wrapper = mount(PluginsPage, { shallow: true });
      await flushPromises();
      await wrapper.find("[data-test='install-com.flickertalk.sketch']").trigger("click");
      await flushPromises();
      expect(nav.push).toHaveBeenCalledWith("/plan");
      expect(wrapper.find("[role='alert']").exists()).toBe(false);
    });
  });
});
