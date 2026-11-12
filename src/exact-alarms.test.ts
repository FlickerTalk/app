import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));

const alerts = vi.hoisted(() => ({ create: vi.fn(), present: vi.fn() }));
vi.mock("@ionic/vue", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@ionic/vue")>()),
  alertController: { create: alerts.create },
}));

import { offerExactAlarms } from "./exact-alarms";

interface Button {
  text: string;
  role?: string;
  handler?: () => unknown;
}
const shown = () => alerts.create.mock.calls.map(([options]) => options as { message: string; buttons: Button[] });

describe("offerExactAlarms", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    alerts.present.mockReset();
    alerts.create.mockReset().mockImplementation(async () => ({ present: alerts.present, onDidDismiss: () => Promise.resolve({}) }));
  });

  it("asks nothing where reminders ring on time already", async () => {
    tauri.invoke.mockResolvedValue(true);
    await offerExactAlarms();
    expect(tauri.invoke).toHaveBeenCalledWith("core_exact_alarms");
    expect(alerts.create).not.toHaveBeenCalled();
  });

  it("asks nothing when the phone cannot say", async () => {
    tauri.invoke.mockRejectedValue(new Error("no bridge"));
    await offerExactAlarms();
    tauri.invoke.mockResolvedValue(undefined);
    await offerExactAlarms();
    expect(alerts.create).not.toHaveBeenCalled();
  });

  it("says in one sentence why, and offers the phone's settings or not now", async () => {
    tauri.invoke.mockResolvedValue(false);
    await offerExactAlarms();
    expect(shown()).toHaveLength(1);
    const [alert] = shown();
    expect(alert.message).toBe("Reminders ring on time only if FlickerTalk may set exact alarms on this phone.");
    expect(alert.buttons.map((button) => button.text)).toEqual(["Not now", "Open Settings"]);
    expect(alert.buttons[0].role).toBe("cancel");
    expect(alerts.present).toHaveBeenCalledTimes(1);
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_request_exact_alarms");
  });

  it("opens the phone's settings when the user asks to", async () => {
    tauri.invoke.mockResolvedValue(false);
    await offerExactAlarms();
    shown()[0].buttons[1].handler?.();
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_request_exact_alarms");
  });

  it("ends once the user has answered", async () => {
    tauri.invoke.mockResolvedValue(false);
    let dismiss = () => {};
    alerts.create.mockImplementation(async () => ({
      present: alerts.present,
      onDidDismiss: () => new Promise((resolve) => (dismiss = () => resolve({}))),
    }));
    let ended = false;
    const offered = offerExactAlarms().then(() => (ended = true));
    await flushPromises();
    expect(alerts.present).toHaveBeenCalled();
    expect(ended).toBe(false);
    dismiss();
    await offered;
    expect(ended).toBe(true);
  });

  it("never fails, even when the alert cannot be shown", async () => {
    tauri.invoke.mockResolvedValue(false);
    alerts.create.mockRejectedValue(new Error("no overlay"));
    await expect(offerExactAlarms()).resolves.toBeUndefined();
  });
});
