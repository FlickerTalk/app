<script setup lang="ts">
import { computed } from "vue";
import { IonPage } from "@ionic/vue";
import { useRoute } from "vue-router";
import CircleThread from "../components/CircleThread.vue";
import { useOnScreen } from "../on-screen";

const route = useRoute();
const circleId = computed(() => String(route.params.id));
const onScreen = useOnScreen();
</script>

<template>
  <ion-page class="ft-thread">
    <CircleThread :circle-id="circleId" :show-back="true" :active="onScreen" />
  </ion-page>
</template>

<style scoped>
.ft-thread {
  background: var(--ft-bg);
}
</style>
