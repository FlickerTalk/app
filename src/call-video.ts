import { computed, ref, watch, watchEffect, type Ref } from "vue";
import { useThumbDrag } from "./call-thumbnail";
import { call, hideVideo, layoutVideo, rectOf, switchCamera, toggleCamera, type VideoLayout } from "./calls";
import { holeClip } from "./present";
import { darkScreen } from "./theme";

export interface CallSurfaces {
  remoteSlot: HTMLElement | null;
  localSlot: HTMLElement | null;
  remoteVideo: HTMLVideoElement | null;
  localVideo: HTMLVideoElement | null;
  remoteAudio: HTMLAudioElement | null;
}

export interface CallVideoOptions {
  current: Readonly<Ref<boolean>>;
  onScreen: Readonly<Ref<boolean>>;
  presentArea: Readonly<Ref<boolean>>;
  body: Readonly<Ref<HTMLElement | null>>;
  surfaces: Readonly<Ref<CallSurfaces | null>>;
  presentBox: Readonly<Ref<HTMLElement | null>>;
  controls: Readonly<Ref<{ $el?: Element } | null>>;
  minimize: Readonly<Ref<{ $el?: Element } | null>>;
  peer: Readonly<Ref<{ $el?: Element } | null>>;
}

export function useCallVideo({ current, onScreen, presentArea, body, surfaces, presentBox, controls, minimize, peer }: CallVideoOptions) {
  const presentClip = ref<string | undefined>();
  const presentBottom = ref<number | undefined>();
  const presentStyle = computed(() => {
    const style: Record<string, string> = {};
    if (presentClip.value) style.clipPath = presentClip.value;
    if (presentBottom.value !== undefined) style.bottom = `${presentBottom.value}px`;
    return Object.keys(style).length ? style : undefined;
  });

  // on phones the pictures are native views under the WebView, this page leaves holes for them
  const native = computed(() => current.value && call.native);
  const live = computed(() => call.phase === "active");
  const cameraReady = computed(() => live.value && call.view.available);
  // before the video line is up a see-through page only shows the window's white
  const cameraRunning = computed(() => cameraReady.value && call.view.camera);
  const stage = computed(() => native.value && live.value && (cameraRunning.value || call.view.remote));
  const showRemote = computed(() => stage.value && call.view.remote);
  const showLocal = computed(() => stage.value && cameraRunning.value && !presentArea.value);
  const localThumb = computed(() => Boolean(showRemote.value && showLocal.value));
  const remoteThumb = computed(() => Boolean(showRemote.value && presentArea.value));
  const invite = computed(() => native.value && cameraReady.value && call.view.remote && !call.view.camera);
  const dark = computed(() => native.value && !stage.value && (call.video || call.view.camera || call.view.remote));
  const cameraUsable = computed(() => cameraReady.value || (native.value && call.view.camera));

  const unavailable = ref(false);
  let unavailableTimer: ReturnType<typeof setTimeout> | undefined;
  function camera() {
    if (!native.value || cameraUsable.value) {
      void toggleCamera();
      return;
    }
    if (!live.value) return;
    unavailable.value = true;
    clearTimeout(unavailableTimer);
    unavailableTimer = setTimeout(() => (unavailable.value = false), 4000);
  }

  function flip() {
    if (cameraRunning.value) void switchCamera();
  }

  const remoteSlot = computed(() => surfaces.value?.remoteSlot ?? null);
  const localSlot = computed(() => surfaces.value?.localSlot ?? null);

  function measure(): VideoLayout {
    return {
      remote: rectOf(remoteSlot.value),
      local: rectOf(localSlot.value),
      mirrorLocal: call.view.facing === "front",
      localRadius: localThumb.value ? 16 : 0,
    };
  }
  let shown = true;
  function measureControls() {
    const screen = rectOf(body.value);
    const buttons = rectOf(controls.value?.$el);
    if (!screen || !buttons || screen.height <= 0 || buttons.height <= 0) return;
    presentBottom.value = Math.max(0, Math.round(screen.y + screen.height - buttons.y));
  }
  function relayout() {
    measureControls();
    presentClip.value = remoteThumb.value ? holeClip(rectOf(presentBox.value), rectOf(remoteSlot.value)) : undefined;
    if (shown && native.value) layoutVideo(measure);
  }
  function hide() {
    shown = false;
    if (native.value) hideVideo();
  }

  const seeThrough = ref(true);
  watchEffect(() => document.documentElement.classList.toggle("ft-call-video", Boolean(stage.value) && seeThrough.value));
  watchEffect(() => darkScreen("call", onScreen.value && Boolean(stage.value || dark.value)));

  watch([native, stage, showRemote, showLocal, remoteThumb, presentArea, () => call.view.facing], relayout, { flush: "post" });
  const sized = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(relayout);
  watch([body, remoteSlot, localSlot, presentBox], (elements) => {
    sized?.disconnect();
    for (const element of elements) if (element) sized?.observe(element);
  });
  function cameIn() {
    shown = true;
    seeThrough.value = true;
    relayout();
  }
  function wentOut() {
    hide();
    seeThrough.value = false;
  }

  const { dragging, thumbStyle, grabThumb, moveThumb, dropThumb, endDrag } = useThumbDrag({
    body,
    remoteSlot,
    localSlot,
    remoteThumb,
    localThumb,
    above: () => [rectOf(minimize.value?.$el), rectOf(peer.value?.$el)],
    moved: relayout,
  });

  const remoteVideo = computed(() => surfaces.value?.remoteVideo ?? null);
  const localVideo = computed(() => surfaces.value?.localVideo ?? null);
  const remoteAudio = computed(() => surfaces.value?.remoteAudio ?? null);
  watchEffect(() => {
    if (remoteVideo.value) remoteVideo.value.srcObject = call.remote;
    if (localVideo.value) localVideo.value.srcObject = call.local;
    if (remoteAudio.value) remoteAudio.value.srcObject = call.remote;
  });

  function attach() {
    window.addEventListener("resize", relayout);
    window.addEventListener("orientationchange", relayout);
    relayout();
  }
  function detach() {
    clearTimeout(unavailableTimer);
    window.removeEventListener("resize", relayout);
    window.removeEventListener("orientationchange", relayout);
    sized?.disconnect();
    endDrag();
    hide();
    seeThrough.value = false;
    document.documentElement.classList.remove("ft-call-video");
    darkScreen("call", false);
  }

  return {
    native,
    stage,
    showRemote,
    showLocal,
    localThumb,
    remoteThumb,
    invite,
    dark,
    cameraUsable,
    cameraRunning,
    unavailable,
    presentStyle,
    dragging,
    thumbStyle,
    camera,
    flip,
    grabThumb,
    moveThumb,
    dropThumb,
    cameIn,
    wentOut,
    attach,
    detach,
  };
}
