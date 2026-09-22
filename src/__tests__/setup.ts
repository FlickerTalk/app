import { config } from "@vue/test-utils";
import { i18n } from "../i18n";

// Unit tests stub Ionic's web components; render their slots so page content stays testable.
config.global.renderStubDefaultSlot = true;
// Components read every visible string from the catalogue.
config.global.plugins = [i18n];

// A Tauri bridge that answers nothing: views can call the core without a Rust side.
import { installTauri } from "./tauri";
installTauri();
