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

  it("joins the room as the caller", async () => {
    const wrapper = await open();
    await wrapper.find("[data-test='connect']").trigger("click");
    expect(tauri.invoke).toHaveBeenCalledWith("poc_connect", { relay: "ws://10.0.2.2:8787", room: "demo" });
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
