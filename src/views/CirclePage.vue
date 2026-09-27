<script setup lang="ts">
import { computed } from "vue";
import { IonPage } from "@ionic/vue";
import { useRoute } from "vue-router";
import CircleThread from "../components/CircleThread.vue";

const route = useRoute();
const circleId = computed(() => String(route.params.id));
</script>

<template>
  <ion-page>
    <CircleThread :circle-id="circleId" :show-back="true" />
  </ion-page>
</template>
