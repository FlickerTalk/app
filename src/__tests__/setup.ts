import { config } from "@vue/test-utils";

// Unit tests stub Ionic's web components; render their slots so page content stays testable.
config.global.renderStubDefaultSlot = true;
