import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  handlers: {} as Record<string, (event: { payload: string }) => void>,
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: string }) => void) => {
    tauri.handlers[name] = handler;
    return Promise.resolve(() => undefined);
  },
}));

import PocPage from "./PocPage.vue";

async function open() {
  const wrapper = mount(PocPage, { shallow: true });
  await flushPromises();
  return wrapper;
}

describe("PocPage", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "poc_default_relay" ? "ws://10.0.2.2:8787" : undefined),
    );
  });

  it("proposes the relay of the developer's machine", async () => {
    const wrapper = await open();
    expect((wrapper.find("[data-test='relay']").element as HTMLInputElement).value).toBe("ws://10.0.2.2:8787");
  });

  it("joins the room as the caller by default", async () => {
    const wrapper = await open();
    await wrapper.find("[data-test='connect']").trigger("click");
    expect(tauri.invoke).toHaveBeenCalledWith("poc_connect", {
      relay: "ws://10.0.2.2:8787",
      room: "demo",
      role: "caller",
      relayOnly: false,
    });
  });

  // Two emulators: one calls, the other answers.
  it("can wait for the call instead", async () => {
    const wrapper = await open();
    await wrapper.find("[data-test='role']").setValue("callee");
    await wrapper.find("[data-test='connect']").trigger("click");
    expect(tauri.invoke).toHaveBeenCalledWith("poc_connect", expect.objectContaining({ role: "callee" }));
  });

  // Plan §17: "Always relay" sends everything through TURN.
  it("can force every packet through the TURN relay", async () => {
    const wrapper = await open();
    await wrapper.find("[data-test='relay-only']").setValue(true);
    await wrapper.find("[data-test='connect']").trigger("click");
    expect(tauri.invoke).toHaveBeenCalledWith("poc_connect", expect.objectContaining({ relayOnly: true }));
  });

  it("shows when the data channel opens and what arrives", async () => {
    const wrapper = await open();
    tauri.handlers["poc://state"]({ payload: "open" });
    tauri.handlers["poc://message"]({ payload: "hello back" });
    await flushPromises();
    expect(wrapper.text()).toContain("Data channel open");
    expect(wrapper.text()).toContain("hello back");
  });

  it("sends hello once the channel is open", async () => {
    const wrapper = await open();
    tauri.handlers["poc://state"]({ payload: "open" });
    await flushPromises();
    await wrapper.find("[data-test='send']").trigger("click");
    expect(tauri.invoke).toHaveBeenCalledWith("poc_send", { text: "hello" });
  });
});
