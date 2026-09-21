<script setup lang="ts">
import { ref, watchEffect } from "vue";
import QRCode from "qrcode";

const props = withDefaults(defineProps<{ value: string; size?: number }>(), { size: 220 });

// The SVG comes from the qrcode library, never from user input.
const svg = ref("");

watchEffect(async () => {
  svg.value = await QRCode.toString(props.value, {
    type: "svg",
    margin: 0,
    errorCorrectionLevel: "M",
  });
});
</script>

<template>
  <!-- eslint-disable-next-line vue/no-v-html -->
  <span
    class="ft-qr"
    role="img"
    :aria-label="$t('qr.label')"
    :style="{ width: `${size}px`, height: `${size}px` }"
    v-html="svg"
  />
</template>

<style scoped>
/* Always black on white: a themed QR is harder for other phones to scan. */
.ft-qr {
  display: block;
  padding: 12px;
  border-radius: 16px;
  background: #fff;
  box-sizing: content-box;
}
.ft-qr :deep(svg) {
  display: block;
  width: 100%;
  height: 100%;
  shape-rendering: crispEdges;
}
</style>
