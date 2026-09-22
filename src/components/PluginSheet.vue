<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { frameUrl, fromFrame } from "../plugins";

// Plan §53, §58: the plugin lives in its own frame, served from its own scheme with the policy its
// permissions allow. It is handed the text the user chose, and nothing else of the chat; it talks
// back only to say that it is ready or how tall it has become.
const props = defineProps<{ plugin: { id: string; name: string }; text: string }>();

const frame = ref<HTMLIFrameElement | null>(null);
const height = ref(120);

function hand() {
  frame.value?.contentWindow?.postMessage({ type: "ft.render", text: props.text }, "*");
}

function onMessage(event: MessageEvent) {
  const said = fromFrame(event, frame.value);
  if (!said) return;
  if (said.type === "ft.ready") hand();
  else if (said.type === "ft.height" && said.height) height.value = Math.min(Math.max(said.height, 80), 2000);
}

onMounted(() => window.addEventListener("message", onMessage));
onBeforeUnmount(() => window.removeEventListener("message", onMessage));
watch(() => props.text, hand);
</script>

<template>
  <section class="ft-plugin">
    <h2 class="ft-plugin__name">{{ plugin.name }}</h2>
    <iframe
      ref="frame"
      class="ft-plugin__frame"
      :src="frameUrl(plugin.id)"
      :style="{ height: `${height}px` }"
      sandbox="allow-scripts"
      referrerpolicy="no-referrer"
      :title="plugin.name"
    />
  </section>
</template>

<style scoped>
.ft-plugin {
  padding: var(--ft-space-4);
}
.ft-plugin__name {
  margin: 0 0 var(--ft-space-3);
  font-size: 15px;
  color: var(--ft-muted);
  font-weight: 600;
}
.ft-plugin__frame {
  display: block;
  width: 100%;
  border: 0;
  background: transparent;
}
</style>
