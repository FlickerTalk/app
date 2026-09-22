<script setup lang="ts">
import { IonIcon } from "@ionic/vue";
import { closeOutline } from "ionicons/icons";
import { cancelScan, scanner } from "../scanner";

// While the camera reads a QR code the app is see-through (the camera is under it): this draws
// the frame and the way out.
</script>

<template>
  <div v-if="scanner.active" class="ft-scanner" data-test="scanner-overlay">
    <span class="ft-scanner__frame" aria-hidden="true" />
    <p class="ft-scanner__hint">{{ $t("addContact.scanHint") }}</p>
    <button type="button" class="ft-round ft-scanner__cancel" :aria-label="$t('common.cancel')" @click="cancelScan">
      <ion-icon :icon="closeOutline" aria-hidden="true" />
    </button>
  </div>
</template>

<style scoped>
.ft-scanner {
  position: fixed;
  z-index: 2000;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 28px;
  padding: calc(env(safe-area-inset-top) + 24px) 24px calc(env(safe-area-inset-bottom) + 24px);
  color: #fff;
}
.ft-scanner__frame {
  width: min(70vw, 300px);
  aspect-ratio: 1;
  border: 3px solid rgba(255, 255, 255, 0.9);
  border-radius: 28px;
  box-shadow: 0 0 0 100vmax rgba(0, 0, 0, 0.45);
}
.ft-scanner__hint {
  margin: 0;
  max-width: 320px;
  text-align: center;
  text-shadow: 0 1px 6px rgba(0, 0, 0, 0.6);
}
.ft-scanner__cancel {
  width: 58px;
  height: 58px;
  font-size: 28px;
  color: #fff;
  background: rgba(0, 0, 0, 0.55);
}
</style>
