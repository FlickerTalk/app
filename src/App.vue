<script setup lang="ts">
import { IonApp, IonRouterOutlet } from "@ionic/vue";
import IncomingCall from "./components/IncomingCall.vue";
</script>

<template>
  <ion-app>
    <ion-router-outlet />
    <IncomingCall />
  </ion-app>
</template>
