<script setup lang="ts">
import { IonApp, IonRouterOutlet } from "@ionic/vue";
import IncomingCall from "./components/IncomingCall.vue";
import ScannerOverlay from "./components/ScannerOverlay.vue";
</script>

<template>
  <ion-app>
    <ion-router-outlet />
    <IncomingCall />
    <ScannerOverlay />
  </ion-app>
</template>
