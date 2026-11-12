import { config } from "@vue/test-utils";
import { i18n } from "../i18n";

config.global.renderStubDefaultSlot = true;
config.global.plugins = [i18n];

import { installTauri } from "./tauri";
installTauri();
