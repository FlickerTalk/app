<script setup lang="ts">
import { IonIcon } from "@ionic/vue";
import { cameraOutline, closeOutline } from "ionicons/icons";
import { closeOnBackWhile } from "../back";
import { cancelScan, dismissRefusal, openCameraSettings, scanner } from "../scanner";

// While the camera reads a QR code the app is see-through (the camera is under it): this draws
// the frame and the way out. On Android the camera draws under the WebView, which keeps the back
// button: Back closes the camera here, or the page underneath went back and the camera stayed on.
closeOnBackWhile(() => scanner.active, () => void cancelScan());

// A refused camera is said out loud, on whichever page asked for it (add a contact, move phones).
closeOnBackWhile(() => Boolean(scanner.refused), dismissRefusal);
</script>

<template>
  <div v-if="scanner.active" class="ft-scanner" data-test="scanner-overlay">
    <span class="ft-scanner__frame" aria-hidden="true" />
    <p class="ft-scanner__hint">{{ $t("addContact.scanHint") }}</p>
    <button type="button" class="ft-round ft-scanner__cancel" :aria-label="$t('common.cancel')" @click="cancelScan">
      <ion-icon :icon="closeOutline" aria-hidden="true" />
    </button>
  </div>
  <div v-else-if="scanner.refused" class="ft-refused" role="alert" data-test="camera-refused">
    <ion-icon :icon="cameraOutline" class="ft-refused__icon" aria-hidden="true" />
    <p class="ft-refused__text">
      {{ scanner.refused === "blocked" ? $t("scanner.cameraBlocked") : $t("scanner.cameraDenied") }}
    </p>
    <button
      v-if="scanner.refused === 'blocked'"
      type="button"
      class="ft-refused__settings"
      data-test="camera-settings"
      @click="openCameraSettings"
    >
      {{ $t("scanner.openSettings") }}
    </button>
    <button type="button" class="ft-round ft-refused__close" :aria-label="$t('common.close')" @click="dismissRefusal">
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

/* The notice of a refused camera: a card at the foot of the screen, over the page that asked. */
.ft-refused {
  position: fixed;
  z-index: 2000;
  inset-inline: 12px;
  bottom: calc(env(safe-area-inset-bottom) + 12px);
  display: grid;
  grid-template-columns: auto 1fr auto;
  align-items: center;
  gap: 8px 12px;
  margin-inline: auto;
  max-width: 480px;
  padding-block: 14px;
  padding-inline: 16px 12px;
  border: 1px solid var(--ft-border);
  border-radius: 18px;
  background: var(--ft-surface);
  color: var(--ft-text);
  box-shadow: 0 8px 30px rgba(0, 0, 0, 0.3);
}
.ft-refused__icon {
  font-size: 26px;
  color: var(--ft-accent);
}
.ft-refused__text {
  margin: 0;
  font-size: 14px;
  line-height: 1.4;
  text-align: start;
}
.ft-refused__close {
  width: 36px;
  height: 36px;
  font-size: 20px;
}
.ft-refused__settings {
  grid-column: 2 / 4;
  justify-self: start;
  appearance: none;
  padding: 10px 18px;
  border: 0;
  border-radius: 12px;
  background: linear-gradient(135deg, var(--ft-accent), var(--ft-accent-2));
  color: var(--ft-on-accent);
  font: inherit;
  font-weight: 600;
  cursor: pointer;
}
</style>
