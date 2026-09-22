import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";

const scanning = vi.hoisted(() => ({ cancelScan: vi.fn() }));
vi.mock("../scanner", async () => {
  const { reactive } = await import("vue");
  return { scanner: reactive({ active: false }), cancelScan: scanning.cancelScan };
});

import ScannerOverlay from "./ScannerOverlay.vue";
import { scanner } from "../scanner";

describe("ScannerOverlay", () => {
  beforeEach(() => {
    scanner.active = false;
    scanning.cancelScan.mockClear();
  });

  it("only shows while the camera reads", () => {
    expect(mount(ScannerOverlay, { shallow: true }).find("[data-test='scanner-overlay']").exists()).toBe(false);
    scanner.active = true;
    expect(mount(ScannerOverlay, { shallow: true }).find("[data-test='scanner-overlay']").exists()).toBe(true);
  });

  // The way out the plugin's own view lacks.
  it("cancels the scan", async () => {
    scanner.active = true;
    await mount(ScannerOverlay, { shallow: true }).find("[aria-label='Cancel']").trigger("click");
    expect(scanning.cancelScan).toHaveBeenCalled();
  });
});
