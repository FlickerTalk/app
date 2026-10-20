<script setup lang="ts">
import { computed } from "vue";
import { IonPage } from "@ionic/vue";
import { useRoute } from "vue-router";
import CircleThread from "../components/CircleThread.vue";
import { useOnScreen } from "../on-screen";

const route = useRoute();
const circleId = computed(() => String(route.params.id));
// Under another page (its settings), what the circle left open lets go of Android's back button.
const onScreen = useOnScreen();
</script>

<template>
  <ion-page class="ft-thread">
    <!-- The circle's header, content and footer are this page's own children (Ionic's shape). -->
    <CircleThread :circle-id="circleId" :show-back="true" :active="onScreen" />
  </ion-page>
</template>

<style scoped>
/* Ionic's page already lays out the circle's header, content and footer in a column. */
.ft-thread {
  background: var(--ft-bg);
}
</style>
