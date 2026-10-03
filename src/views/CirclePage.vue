<script setup lang="ts">
import { computed, ref } from "vue";
import { IonPage, onIonViewDidEnter, onIonViewWillLeave } from "@ionic/vue";
import { useRoute } from "vue-router";
import CircleThread from "../components/CircleThread.vue";

const route = useRoute();
const circleId = computed(() => String(route.params.id));
// Under another page (its settings), what the circle left open lets go of Android's back button.
const onScreen = ref(true);
onIonViewWillLeave(() => (onScreen.value = false));
onIonViewDidEnter(() => (onScreen.value = true));
</script>

<template>
  <ion-page>
    <CircleThread :circle-id="circleId" :show-back="true" :active="onScreen" />
  </ion-page>
</template>
