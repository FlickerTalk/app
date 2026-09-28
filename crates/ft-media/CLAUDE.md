# app/crates/ft-media

Media nativa de las llamadas (2026-09-28): la **voz** de una llamada en Rust, sobre
`webrtc-engine` (repo `FlickerTalk/webrtc-engine-rs`, rama `audio-engine`, dependencia git). El
núcleo (`ft-core/src/native_calls.rs`) decide cuándo; este crate sabe cómo.

- `MediaSession`: la conexión de una llamada de voz (solo Opus), oferta y respuesta enteras con sus
  candidatos (sin goteo, como los DataChannels). A una oferta con vídeo (un WebView) se le contesta
  solo el audio: el motor solo conoce Opus y rechaza la línea de vídeo (`m=video 0`).
- `ice_setup` aplica la ruta de Ajustes (`§17`): `direct` sin TURN, `auto` todo, `always` solo TURN
  con política `relay`.
- `Voice`: el dispositivo y la cadena del motor (Opus, RTP, jitter buffer). Corre solo con la
  llamada conectada y, en iOS (`Activation::WhenSessionActive`), con la sesión de audio de CallKit
  activa; si se retira y vuelve, arranca de nuevo con anillos nuevos. El dispositivo vive en un hilo
  propio (un backend nunca cruza de hilo) y recibe `maintain()` cada 100 ms (Android reabre ahí sus
  flujos tras un cambio de auriculares). Primero se para el dispositivo y después la cadena.
- `platform_audio()`: el backend del motor (`platform_backend`: VoiceProcessingIO en iOS, AAudio en
  Android); en escritorio no hay y las llamadas siguen en el WebView.
- Feature `testing`: `ToneDevice` (habla una voz de prueba en tiempo real y graba lo que suena),
  `mean_heard` (correlación) y la oferta de vídeo de un WebView, para los tests de `ft-core`.

Es la única pieza con código por plataforma entre los crates: los `cfg` de `platform.rs` eligen el
backend del motor; nada de Kotlin ni Swift.
