import { beforeEach, describe, expect, it, onTestFinished, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { computed, effectScope, ref } from "vue";
import type { Router } from "vue-router";
import { seed } from "./__tests__/seed";
import { installTauri } from "./__tests__/tauri";
import { actions, call, resetCalls } from "./__tests__/calls-mock";
import { refreshPlugins } from "./plugins";
import { useCallPresenting } from "./call-presenting";

vi.mock("./calls", async () => (await import("./__tests__/calls-mock")).callsMock());
vi.mock("@tauri-apps/api/app", () => ({ onBackButtonPress: async () => ({ unregister: async () => undefined }) }));

const BOARD = "com.flickertalk.board";
const tool = (id: string, name: string, live: boolean) => ({
  id,
  name,
  version: "1.1.0",
  installedAt: 1,
  asks: { network: [], messages: false, send: "nothing", live: true },
  granted: { network: [], messages: false, send: "nothing", live },
});
const active = (patch: Record<string, unknown> = {}) =>
  Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now(), native: true, canPresent: true, ...patch });

let tools: unknown[] = [];

async function setUp() {
  await refreshPlugins();
  const push = vi.fn();
  const id = computed(() => "c1");
  const contact = computed(() => ({ name: "Maria López", hue: 0 }));
  const current = computed(() => call.contact === "c1" && call.phase !== "idle");
  const onScreen = ref(true);
  const scope = effectScope();
  const presenting = scope.run(() => useCallPresenting({ id, contact, current, onScreen, router: { push } as unknown as Router, sheet: ref(null) }))!;
  onTestFinished(() => scope.stop());
  await flushPromises();
  return { presenting, push };
}

describe("useCallPresenting", () => {
  beforeEach(() => {
    seed();
    resetCalls();
    tools = [tool(BOARD, "Board", true)];
    installTauri((command) => {
      if (command === "core_plugins") return tools;
      if (command === "core_plan") return { state: "trial", until: 0 };
      if (command === "core_catalogue") return [];
      if (command === "core_conversations" || command === "core_requests" || command === "core_messages") return [];
      return undefined;
    });
  });

  it("offers to present only on an active call of a phone that can, with nothing presented yet", async () => {
    active({ canPresent: false });
    const { presenting } = await setUp();
    expect(presenting.canOffer.value).toBe(false);
    call.canPresent = true;
    expect(presenting.canOffer.value).toBe(true);
    call.phase = "connecting";
    expect(presenting.canOffer.value).toBe(false);
    call.phase = "active";
    call.presenting = { plugin: BOARD, by: "them" };
    expect(presenting.canOffer.value).toBe(false);
  });

  it("presents the whiteboard at once when it may", async () => {
    active();
    const { presenting } = await setUp();
    presenting.choosing.value = true;
    presenting.present(BOARD);
    expect(presenting.choosing.value).toBe(false);
    await flushPromises();
    expect(actions.presentInCall).toHaveBeenCalledWith("x", BOARD);
  });

  it("asks before presenting with a tool that lacks the live channel, and drops that question when closing its own sheets", async () => {
    active();
    tools = [tool(BOARD, "Board", false)];
    const { presenting } = await setUp();
    presenting.present(BOARD);
    await flushPromises();
    expect(presenting.asking.value).toMatchObject({ them: false, name: "Board" });
    expect(actions.presentInCall).not.toHaveBeenCalled();
    presenting.choosing.value = true;
    presenting.closeOwnSheets();
    expect(presenting.choosing.value).toBe(false);
    expect(presenting.asking.value).toBeNull();
  });

  it("asks about their presentation of a tool not yet allowed, and keeps that question when closing its own sheets", async () => {
    tools = [tool(BOARD, "Board", false)];
    active({ presenting: { plugin: BOARD, by: "them" } });
    const { presenting } = await setUp();
    expect(presenting.presentState.value).toBe("ask");
    expect(presenting.asking.value).toMatchObject({ them: true, body: "Maria López wants to present with Board" });
    presenting.closeOwnSheets();
    expect(presenting.asking.value).toMatchObject({ them: true });
  });

  it("says the tool is missing once this phone's plugins are read and nothing offers it", async () => {
    tools = [];
    active({ presenting: { plugin: BOARD, by: "them" } });
    const { presenting } = await setUp();
    expect(presenting.presentState.value).toBe("missing");
    expect(presenting.presentArea.value).toBe(false);
  });

  it("opens the presented tool over the call, leading when the presentation is mine", async () => {
    active({ presenting: { plugin: BOARD, by: "me" } });
    const { presenting } = await setUp();
    expect(presenting.presentState.value).toBe("sheet");
    expect(presenting.presentArea.value).toBe(true);
    expect(presenting.onSheet.value).toMatchObject({ plugin: { id: BOARD, name: "Board" }, lead: true });
  });

  it("stops presenting when the presenter's tool asks to close, and only then", async () => {
    active({ presenting: { plugin: BOARD, by: "them" } });
    const { presenting } = await setUp();
    presenting.presentDone();
    expect(actions.stopPresenting).not.toHaveBeenCalled();
    call.presenting = { plugin: BOARD, by: "me" };
    presenting.presentDone();
    expect(actions.stopPresenting).toHaveBeenCalledWith("x");
  });
});
