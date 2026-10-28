import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { IonButton, IonIcon, IonItem } from "@ionic/vue";
import { constructOutline, locationOutline } from "ionicons/icons";
import PermissionAsk from "./PermissionAsk.vue";
import source from "./PermissionAsk.vue?raw";
import { IonModalStub } from "../__tests__/ionic";
import en from "../i18n/en.json";

const LOCATION = { label: "Your location, only when you ask", icon: locationOutline };
const catalogues = import.meta.glob<{ plugins: { allow: string } }>("../i18n/*.json", { eager: true, import: "default" });
type Stubs = NonNullable<NonNullable<Parameters<typeof mount>[1]>["global"]>["stubs"];
const sheet = (props: Record<string, unknown>, stubs: Stubs = { IonModal: IonModalStub, IonIcon: true }) =>
  mount(PermissionAsk, { props: { open: true, name: "Location", permission: LOCATION, ...props }, global: { stubs } });

describe("PermissionAsk", () => {
  // Decision of 2026-10-08: a tool that lacks a permission asks for it on the spot, in Ionic's
  // sheet modal, as the game's permissions sheet: the tool, the one permission and Cancel / Continue.
  it("is an Ionic sheet that names the tool and the one permission it needs", async () => {
    const wrapper = sheet({});
    const modal = wrapper.findComponent(IonModalStub);
    expect(modal.props("isOpen")).toBe(true);
    expect(modal.props("breakpoints")).toContain(modal.props("initialBreakpoint"));
    expect(modal.attributes("aria-label")).toBe("Location needs a permission");
    // Ionic's components are Stencil "scoped" elements: in happy-dom their text is only in the HTML.
    const html = wrapper.find("[data-test='permission-ask']").html();
    expect(html).toContain("Location needs a permission");
    const line = wrapper.findComponent(IonItem);
    expect(line.attributes("data-test")).toBe("permission-asked");
    expect(line.html()).toContain("Your location, only when you ask");
    const allow = wrapper.findAllComponents(IonButton).find((one) => one.attributes("data-test") === "permission-allow")!;
    expect(allow.element.innerHTML).toContain("Continue");
    expect(allow.element.innerHTML).not.toContain("Allow");
    const cancel = wrapper.findAllComponents(IonButton).find((one) => one.attributes("data-test") === "permission-cancel")!;
    expect(cancel.element.innerHTML).toContain("Cancel");

    await wrapper.find("[data-test='permission-allow']").trigger("click");
    await wrapper.find("[data-test='permission-cancel']").trigger("click");
    expect(wrapper.emitted("allow")).toHaveLength(1);
    expect(wrapper.emitted("cancel")).toHaveLength(1);
  });

  it("shows the permission's icon, and the tool's own icon or image", () => {
    const icons = sheet({ icon: constructOutline }, { IonModal: IonModalStub })
      .findAllComponents(IonIcon)
      .map((one) => one.props("icon"));
    expect(icons).toEqual([constructOutline, locationOutline]);

    const wrapper = sheet({ image: "data:image/svg+xml;base64,PHN2Zy8+" }, { IonModal: IonModalStub });
    const img = wrapper.find("[data-test='permission-ask'] img");
    expect(img.attributes("src")).toBe("data:image/svg+xml;base64,PHN2Zy8+");
    expect(img.attributes("alt")).toBe("");
    expect(wrapper.findAllComponents(IonIcon).map((one) => one.props("icon"))).toEqual([locationOutline]);
  });

  // Dragged down, a tap outside or the back button: Ionic dismisses it, and that is a no.
  it("is a no when Ionic dismisses it, and only one answer", async () => {
    const wrapper = sheet({});
    wrapper.findComponent(IonModalStub).vm.$emit("didDismiss");
    expect(wrapper.emitted("cancel")).toHaveLength(1);
    expect(wrapper.emitted("allow")).toBeUndefined();

    const allowed = sheet({});
    await allowed.find("[data-test='permission-allow']").trigger("click");
    allowed.findComponent(IonModalStub).vm.$emit("didDismiss");
    expect(allowed.emitted("allow")).toHaveLength(1);
    expect(allowed.emitted("cancel")).toBeUndefined();
  });

  // App Review guideline 5.1.1(iv) (2026-10-28): a message shown before the system's own
  // permission request (the location a tool asks for, say) may only go on with "Continue" or
  // "Next", never "Allow": the phone itself asks right after. In every language.
  it("goes on with Continue, never Allow, in every language", () => {
    expect(en.plugins.allow).toBe("Continue");
    const allow =
      /\ballow\b|permitir|erlauben|autoriser|consenti|izinkan|zezw[oó]l|\bpermite\b|разрешить|дозволити|[iİ]zin ver|cho phép|อนุญาต|السماح|অনুমতি দিন|अनुमति दें|許可|허용|允许|允許/i;
    for (const [path, catalogue] of Object.entries(catalogues)) {
      expect(catalogue.plugins.allow, path).not.toMatch(allow);
    }
  });

  it("shows nothing while closed", () => {
    expect(sheet({ open: false }).find("[data-test='permission-ask']").exists()).toBe(false);
  });

  // The same room under the grab handle and above the navigation bar as the game's sheet.
  it("leaves the app sheet's room under the handle and above the bar", () => {
    const styles = source.slice(source.indexOf("<style"));
    expect(styles).toMatch(/\.ft-permission-ask__body\s*\{[^}]*padding-top:\s*26px/);
    expect(styles).toMatch(/\.ft-permission-ask__body\s*\{[^}]*--ion-safe-area-bottom/);
  });
});
