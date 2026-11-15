<script setup lang="ts">
import { ref } from "vue";
import { IonButton, IonIcon, IonItem, IonLabel } from "@ionic/vue";
import { chatbubblesOutline, chevronDownOutline, chevronUpOutline, gameControllerOutline } from "ionicons/icons";
import type { ChatMessage } from "../core";

defineProps<{ peeking: boolean; name: string; last?: ChatMessage }>();
const emit = defineEmits<{ peek: [] }>();

const button = ref<InstanceType<typeof IonButton> | null>(null);
function focus() {
  (button.value?.$el as HTMLElement | undefined)?.focus();
}
defineExpose({ focus });
</script>

<template>
  <ion-item button :detail="false" lines="full" class="ft-room__strip" data-test="game-strip" @click="emit('peek')">
    <ion-icon slot="start" :icon="peeking ? gameControllerOutline : chatbubblesOutline" aria-hidden="true" />
    <ion-label v-if="peeking" class="ion-text-nowrap">{{ name }}</ion-label>
    <ion-label v-else-if="last" class="ion-text-nowrap" dir="auto">{{ last.kind === "file" ? last.file?.name : last.text }}</ion-label>
    <ion-label v-else class="ion-text-nowrap" color="medium">{{ $t("games.showChat") }}</ion-label>
    <ion-button
      ref="button"
      slot="end"
      fill="clear"
      size="default"
      data-test="game-peek"
      :aria-label="peeking ? $t('games.showGame') : $t('games.showChat')"
      @click.stop="emit('peek')"
    >
      <ion-icon slot="icon-only" :icon="peeking ? chevronDownOutline : chevronUpOutline" aria-hidden="true" />
    </ion-button>
  </ion-item>
</template>
