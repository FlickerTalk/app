import { config } from "@vue/test-utils";
import { i18n } from "../i18n";

// Unit tests stub Ionic's web components; render their slots so page content stays testable.
config.global.renderStubDefaultSlot = true;
// Components read every visible string from the catalogue.
config.global.plugins = [i18n];
