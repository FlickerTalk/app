# Vídeo nativo y cambio entre voz y vídeo en cualquier momento

Diseño del 2026-09-29, rama `native-video` (desde `native-calls`). Requisito de Ioan: «en todo
momento se debe poder cambiar entre vídeo y voz», en los dos sentidos, en iOS y Android, también
desde la pantalla de llamada del sistema cuando la plataforma lo permita. Con esto el vídeo de las
llamadas deja el WebView en los teléfonos y pasa a Rust (`webrtc-engine`, rama `main`, que ya
junta audio y vídeo), como la voz desde `native-calls`.

El **commit de contrato** de esta rama deja compilando lo que comparten los trabajos en paralelo
(tipos del protocolo, firmas de `ft-media`, API Rust del puente). Lo demás lo hacen los cuatro
trabajos de la sección «Reparto».

## Resumen de las decisiones

1. **Sin renegociación.** Toda llamada nativa negocia desde la oferta una línea de audio y otra
   de vídeo (`sendrecv`, H.264), empiece como voz o como vídeo. Cambiar es **encender o apagar la
   cámara propia** y decírselo al otro con un paquete nuevo, `CallMedia`, que es un estado (no
   una orden). Así no hay oferta nueva, ni *glare*, ni nada que negociar con el iPhone bloqueado.
   Es lo que hace Signal (RingRTC).
2. **Cada lado manda en su cámara.** Encender la mía no enciende la del otro: ve mi vídeo y se le
   ofrece encender la suya. Nadie abre la cámara de otro.
3. **Compatibilidad por versión de media**: `CallOffer` y `CallAnswer` llevan `media`
   (`CALL_MEDIA_VERSION = 1`); una app anterior no lo manda (se lee 0) y lo ignora al recibirlo.
   Solo con `media ≥ 1` en los dos lados se habla `CallMedia`.
4. **El vídeo vive en `ft-media`** (`Video`, junto a `Voice`), sobre el `VideoCall` del motor y
   la misma conexión (`MediaSession`) de la llamada.
5. **Las vistas nativas van debajo del WebView**, que se vuelve transparente donde hay vídeo: los
   controles siguen siendo HTML, con su i18n y su accesibilidad. El WebView dice dónde va cada
   imagen con rectángulos en píxeles CSS.
6. **La cámara se retiene (`paused`)** cuando la app no está en pantalla, el teléfono está
   bloqueado o se sale de la pantalla de llamada; vuelve sola al regresar. En iOS es obligado (el
   sistema no deja usar la cámara en segundo plano); en Android se hace igual por coherencia y
   privacidad.
7. **CallKit**: `hasVideo` se actualiza en cada cambio (`reportCall(with:updated:)`); el botón
   «Vídeo» de la pantalla del sistema abre la app, y eso enciende la cámara. En Android, la
   notificación de llamada en curso gana una acción de cámara y el servicio en primer plano añade
   el tipo `camera` mientras la cámara está encendida.
8. **Despliegue**: en iOS y Android todas las llamadas pasan a ser nativas (voz y vídeo); el
   escritorio sigue con el WebView y sin cambio de modo (fase 2, receta abajo).

## Investigación (fuentes consultadas el 2026-09-29)

Etiquetas: **[DOC]** documentación oficial, **[SRC]** código fuente leído, **[FORO]** foros o
issues, **[INF]** deducción nuestra, sin fuente que lo diga.

### CallKit y el vídeo en apps VoIP de terceros

- **`CXCallUpdate.hasVideo`** [DOC]: «whether the call includes video in addition to audio», y
  puede cambiar durante la llamada: «a call may be upgraded from audio only to audio and video,
  which would be reflected by a new CXCallUpdate object with its hasVideo property set to true».
  Las actualizaciones de una llamada en curso van por `reportCall(with:updated:)`.
  - https://developer.apple.com/documentation/callkit/cxcallupdate/hasvideo
  - https://developer.apple.com/documentation/callkit/cxcallupdate
  - https://developer.apple.com/documentation/callkit/cxprovider/reportcall(with:updated:)
- **`CXProviderConfiguration.supportsVideo`** [DOC]: si el proveedor admite vídeo además de
  audio; por defecto `false` (nosotros ya lo ponemos a `true`). Apple no documenta qué elementos
  de la interfaz cambia. https://developer.apple.com/documentation/callkit/cxproviderconfiguration/supportsvideo
- **No existe ninguna acción de vídeo** [DOC]: las `CXCallAction` son solo Answer, End, PlayDTMF,
  SetGroup, SetHeld, SetMuted, SetTranslating y Start.
  https://developer.apple.com/documentation/callkit/cxcallaction
- **El botón de la pantalla del sistema** [DOC]: `iconTemplateImageData` se usa «for the button
  which takes the user from this system UI to the 3rd-party app», es decir, abre la app.
  https://developer.apple.com/documentation/callkit/cxproviderconfiguration/icontemplateimagedata
- **El botón «Vídeo»** no llega a ningún delegado de `CXProvider` [DOC por omisión]. Lo que se
  sabe:
  - [FORO] `callservicesd` levanta la app con una *user activity* de intención de llamada
    (`INStartVideoCallIntent` en apps que no declaran `INStartCallIntent`), que se atiende en
    `continueUserActivity`. https://github.com/Actual-Chat/actual-chat/pull/4879 y
    https://github.com/Actual-Chat/actual-chat/issues/4872
  - [SRC] Signal-iOS atiende `INStartVideoCallIntent`, `INStartAudioCallIntent` e
    `INStartCallIntent` (con `callCapability == .videoCall`) como *user activities*:
    https://github.com/signalapp/Signal-iOS/blob/main/Signal/AppLaunch/AppLifecycleManager.swift
  - [FORO] Apple DTS: tras contestar en la pantalla bloqueada, la única forma de que la app se
    abra al desbloquear es marcar la llamada como de vídeo:
    https://developer.apple.com/forums/thread/798090 . En algunos equipos el botón aparece
    desactivado tras contestar bloqueado; DTS lo considera un error si pasa también con su
    ejemplo Speakerbox: https://developer.apple.com/forums/thread/800191
  - **No documentado**: si el botón exige siempre Face ID o código antes de abrir la app. Que la
    app pase a primer plano lo implica [INF].
- **Cámara con el teléfono bloqueado o la app en segundo plano** [DOC]:
  `videoDeviceNotAvailableInBackground`, «Camera usage is prohibited while in the background»; la
  sesión queda interrumpida y arranca sola al volver a primer plano.
  https://developer.apple.com/documentation/avfoundation/avcapturesession/interruptionreason/videodevicenotavailableinbackground
  Conclusión [INF]: una llamada contestada en la pantalla bloqueada no puede mandar vídeo hasta
  que el usuario desbloquea y la app está delante. Signal aplica la misma regla
  (`shouldHaveLocalVideoTrack` es `false` con `applicationState == .background`):
  https://github.com/signalapp/Signal-iOS/blob/main/Signal/Calls/CallService.swift
- **Cámara en multitarea (PiP, Split View)**, para una fase posterior [DOC]:
  `isMultitaskingCameraAccessSupported` es cierto, entre otros casos, si la app enlaza contra
  iOS 18 o posterior y tiene `voip` en `UIBackgroundModes` (lo tenemos), o con la entitlement
  `com.apple.developer.avfoundation.multitasking-camera-access`. `isMultitaskingCameraAccessEnabled`
  va a `true` antes de `startRunning()`. `AVPictureInPictureVideoCallViewController` existe desde
  iOS 15; la ventana PiP no recibe toques y la cámara se interrumpe si se esconde. Nada de esto
  da cámara con el teléfono bloqueado [INF].
  - https://developer.apple.com/documentation/avfoundation/avcapturesession/ismultitaskingcameraaccesssupported
  - https://developer.apple.com/documentation/avfoundation/avcapturesession/ismultitaskingcameraaccessenabled
  - https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.avfoundation.multitasking-camera-access
  - https://developer.apple.com/documentation/avkit/adopting-picture-in-picture-for-video-calls

**Respuesta corta**: CallKit no tiene acción para cambiar a vídeo. `hasVideo` se puede cambiar en
cualquier momento con `reportCall(with:updated:)`. El botón «Vídeo» de la pantalla del sistema
solo abre la app (con una *user activity* de intención de llamada de vídeo, según foros y el
código de Signal, no según Apple), y la cámara solo funciona con la app delante y el teléfono
desbloqueado.

### Cómo cambian WhatsApp y Signal

- **WhatsApp**: durante una llamada de voz se puede *pedir* pasar a vídeo, y el otro elige
  «Cambiar» o «Rechazar» (texto del centro de ayuda obtenido del extracto del buscador; la página
  llegó truncada). https://faq.whatsapp.com/1862285217468140/?cms_platform=mac-desktop y
  https://www.business-standard.com/article/technology/in-middle-of-a-whatsapp-voice-call-you-can-switch-to-video-call-instantly-118011000263_1.html
  El mecanismo interno no es público.
- **Signal / RingRTC** [SRC]:
  - **Siempre negocia audio y vídeo y nunca renegocia.** Cada llamada crea la pista de vídeo
    desactivada (comentario «We always negotiate for both audio and video streams»):
    https://github.com/signalapp/ringrtc/blob/main/src/ios/SignalRingRTC/SignalRingRTC/CallManager.swift
  - Encender o apagar: `setLocalVideoEnabled` → activa la pista y la captura → `set_video_enable`
    → `SenderStatus { video_enabled }`, que viaja por RTP de datos (protobuf) y se repite cada
    segundo. https://github.com/signalapp/ringrtc/blob/main/src/ios/SignalRingRTC/SignalRingRTC/CallContext.swift ,
    https://github.com/signalapp/ringrtc/blob/main/src/rust/src/ios/call_manager.rs ,
    https://github.com/signalapp/ringrtc/blob/main/src/rust/src/core/connection.rs ,
    https://github.com/signalapp/ringrtc/blob/main/protobuf/protobuf/rtp_data.proto
  - El otro lado recibe `RemoteVideoEnable/Disable` y muestra o quita la imagen:
    https://github.com/signalapp/ringrtc/blob/main/src/rust/src/core/call_fsm.rs ,
    https://github.com/signalapp/Signal-iOS/blob/main/Signal/Calls/IndividualCallService.swift
  - El permiso de cámara se pide solo al encender (`updateIsLocalVideoMuted`), y no hay paso de
    aceptar o rechazar: el vídeo del otro simplemente aparece [INF del código].
  - **Actualiza `hasVideo` en CallKit** al encender o apagar la cámara propia
    (`reportCall(with:updated:)`, `update.hasVideo = hasLocalVideo`):
    https://github.com/signalapp/Signal-iOS/blob/main/Signal/Calls/UserInterface/CallKitCallUIAdaptee.swift

### Android

- **CallStyle** [SRC]: `NotificationCompat.CallStyle.setIsVideo` existe («may affect the icons or
  text used on the required action buttons»). No hay API para cambiarla en vivo: Signal vuelve a
  publicar la notificación en curso con `forOngoingCall(...).setIsVideo(...)`.
  - https://github.com/androidx/androidx/blob/androidx-main/core/core/src/main/java/androidx/core/app/NotificationCompat.java
  - https://github.com/signalapp/Signal-Android/blob/main/app/src/main/java/org/thoughtcrime/securesms/webrtc/CallNotificationBuilder.java
  - https://github.com/signalapp/Signal-Android/blob/main/app/src/main/java/org/thoughtcrime/securesms/service/webrtc/ActiveCallManager.kt
- **Servicio en primer plano de tipo `camera`** [DOC]: pide `FOREGROUND_SERVICE_CAMERA` y el
  permiso `CAMERA` concedido; no se puede crear desde segundo plano salvo excepciones, y una de
  ellas es **la interacción del usuario con una notificación**. Para añadir un tipo a un servicio
  que ya corre se vuelve a llamar a `startForeground()` con la máscara completa; todo tipo debe
  estar en el manifiesto.
  - https://developer.android.com/develop/background-work/services/fgs/service-types
  - https://developer.android.com/develop/background-work/services/fgs/restrictions-bg-start
  - https://developer.android.com/develop/background-work/services/fgs/launch
  - https://developer.android.com/about/versions/11/privacy/foreground-services
  - **No documentado**: si añadir `camera` con un segundo `startForeground()` desde segundo plano
    falla igual; muy probablemente sí [INF].
  - Signal declara `camera` en su servicio y lo añade siempre que el permiso está concedido, sea
    o no de vídeo la llamada: https://github.com/signalapp/Signal-Android/blob/main/app/src/main/AndroidManifest.xml
- **Telecom** [SRC]: Core-Telecom tiene `CallControlScope.requestCallType(CALL_TYPE_AUDIO_CALL |
  CALL_TYPE_VIDEO_CALL)` y `callTypeFlow()`, pero solo en 1.1.0-alpha02 a 1.1.0-beta01, no en la
  estable 1.0.1. No usamos Telecom todavía (pendiente de `native-calls`).
  - https://github.com/androidx/androidx/blob/androidx-main/core/core-telecom/src/main/java/androidx/core/telecom/CallControlScope.kt
  - https://dl.google.com/android/maven2/androidx/core/core-telecom/maven-metadata.xml
  - https://github.com/aosp-mirror/platform_frameworks_base/blob/main/telecomm/java/android/telecom/CallControl.java

### WebRTC

- `replaceTrack()` cambia o quita (`null`) la pista de un emisor **sin renegociar** [DOC]:
  https://developer.mozilla.org/en-US/docs/Web/API/RTCRtpSender/replaceTrack
- Cambiar `transceiver.direction` **sí** exige renegociar (`negotiationneeded`) [DOC]:
  https://developer.mozilla.org/en-US/docs/Web/API/RTCRtpTransceiver/direction y RFC 8829 §4.2.3,
  https://www.rfc-editor.org/rfc/rfc8829.html
- Para el *glare* de una renegociación, el patrón *perfect negotiation* (un lado cortés que
  retrocede su oferta): https://developer.mozilla.org/en-US/docs/Web/API/WebRTC_API/Perfect_negotiation

## Decisiones

### 1. Negociación: línea de vídeo siempre, sin renegociar

**Decisión.** Toda oferta nativa lleva `m=audio` (Opus) y `m=video` (H.264 Constrained Baseline
`42e01f`, `packetization-mode=1`, CVO), las dos `sendrecv`, y `media: 1`. Toda respuesta nativa
contesta las líneas que traiga la oferta: a una con vídeo, vídeo `sendrecv` (también a la de una app
anterior). La cámara no se abre por tener la línea: sin cámara encendida el motor no manda nada y
el otro no recibe paquetes de vídeo.

**Cambiar** es encender o apagar la cámara propia y mandar
`CallMedia { call, seq, video, paused }` por la conexión directa cifrada con Olm, igual que
`CallOffer`, `CallAnswer` y `CallEnd` (nunca por el buzón).

**Por qué**:

- **Sin renegociación no hay *glare*.** Que los dos cambien a la vez son dos estados
  independientes, uno por cámara: no hay ofertas que chocar. Con renegociación habría que
  resolverlo (lado cortés, *rollback*) sobre `webrtc-rs`, a la vez con el WebView de una app
  anterior, y con el iPhone bloqueado y su socket al router quizá muerto.
- **Cambio instantáneo**: no hay ida y vuelta de SDP, ni candidatos que juntar (3 s de límite).
- **Probado a escala**: es el modelo de Signal.
- **Coste pequeño**: unos cientos de bytes más de SDP y un transceptor sin tráfico.

**¿Mensaje nuevo? Sí, `CallMedia`, pero no de renegociación.** No hacen falta `CallRenegotiate` ni
`CallOffer` repetido con el mismo id. Si algún día hiciera falta renegociar (reinicio de ICE,
compartir pantalla con una pista nueva), el diseño reservado es
`CallRenegotiate { call, seq, sdp }` / `CallRenegotiateAnswer { call, seq, sdp }`, con el
**llamado como lado cortés** (retrocede su oferta si choca) y `seq` para descartar lo viejo. **No
se implementa ahora.**

**Versión de media y compatibilidad (CBOR)**:

- `CallOffer` y `CallAnswer` ganan `#[serde(default)] media: u16`. Una app anterior no lo manda y
  se lee 0 (test `a_call_offer_or_answer_without_a_media_version_reads_as_version_zero`). Una app
  anterior lee la oferta nueva ignorando el campo, como hace con todo campo que no conoce (test
  `an_older_app_reads_the_new_offer_and_answer_and_ignores_the_camera_state`).
- `CallMedia` es un tipo nuevo de `Body`: una app anterior lo decodifica como `Unknown` y lo
  ignora. Una app más nueva puede añadirle campos, que esta ignora (test
  `a_camera_state_with_fields_from_a_newer_app_is_still_read`).
- Solo se manda `CallMedia` si el otro dijo `media ≥ 1` (en su oferta o su respuesta).

| Otro lado                                      | Qué pasa                                                                    |
| ---------------------------------------------- | --------------------------------------------------------------------------- |
| Nativo nuevo (`media: 1`)                      | Cambio libre en los dos sentidos.                                           |
| App anterior, llamada de **voz** (sin `m=video`) | No hay línea de vídeo: el botón de vídeo sale desactivado (`available: false`). |
| App anterior, llamada de **vídeo**             | Vídeo desde el principio. Apagar la cámara funciona, pero el otro se queda con la última imagen (no entiende `CallMedia`). Volver a encenderla funciona. |
| App anterior que recibe nuestra oferta de voz con `m=video` | Su WebView contesta el vídeo `recvonly` y no lo muestra; su respuesta viene sin `media`: botón desactivado. |
| Rama `native-calls` sin publicar (contesta `m=video 0`) | Como una app anterior de voz. Solo afecta a teléfonos de prueba. |

**Orden y pérdidas de `CallMedia`.** Es un estado, así que gana el de `seq` más alto por llamada y
remitente (lo repetido o tardío no cambia nada). Se manda en cada cambio y, si no sale, se
reintenta cada 2 s mientras dure la llamada, como `deliver_call_end`. Como red de seguridad, el
receptor da por encendida la cámara del otro si le llega vídeo (el primer fotograma) sin
`CallMedia` que diga lo contrario.

### 2. Dónde vive la media de vídeo: `ft-media::Video`

**Decisión.** `ft-media` gana `Video`, hermano de `Voice`, con el `VideoCall` del motor sobre la
misma `MediaSession`. Un solo `PeerConnection`, un solo ICE, un solo DTLS. El motor ya registra
H.264 y sus interceptores en `peer_connection_builder()`, y `VideoCall` es independiente del audio
(«both can run on the same peer connection»).

- `MediaSession::open` añade siempre la pista de vídeo (`add_video_track`). `on_track` reparte por
  tipo: el audio va al `oneshot` de `Voice` y el vídeo a otro para `RemoteVideo::pending`.
- **Dispositivos por llamada**: la fábrica de la plataforma (`VideoPlatform::devices`) se llama
  **una vez**, la primera vez que la llamada necesita vídeo (propio o del otro). La cámara y la
  pantalla, y sus vistas nativas, viven hasta que acaba la llamada. En iOS las capas no cambian a
  mitad de llamada, y en Android las superficies se enchufan una vez.
- **`VideoCall` corre** mientras la llamada está conectada y hay alguna cámara (`state.any()`), y
  se para (conservando los dispositivos) cuando ya no hay ninguna.
  - `VideoCall::start` arranca siempre la fuente. Para que la imagen del otro no abra nuestra
    cámara, `ft-media` envuelve la fuente en una **fuente con compuerta** (`start` no abre nada
    mientras la compuerta está cerrada) y hace `pause()` justo tras arrancar.
  - Mejora opcional en el motor, aparte: `VideoCallConfig { paused: bool }`.
  - Cámara encendida y sin retener: `resume()`. Apagada o retenida: `pause()`. Cambio de cámara:
    `VideoCall::switch_camera`.
- **Acceso a las vistas**: el motor mete fuente y pantalla en cajas dentro de `VideoCall`, así que
  la fábrica de la plataforma las envuelve en `Arc<Mutex<…>>` compartidos. Esos envoltorios
  implementan `VideoSource`/`VideoSink` delegando, y el registro `views` guarda el otro extremo
  para `set_surface`, las capas y la orientación.
- **Forma de la imagen remota**: `Video` lee cada 250 ms `DisplaySink::rotation()` (iOS) o
  `video_size()` (Android) y publica `VideoState.shape` cuando cambia.
- **Estado**: un `watch<VideoState>` (instantánea completa, nunca deltas), que el núcleo convierte
  en `CallUpdate::Video`.

### 3. Vistas nativas

**Decisión: debajo del WebView, que se vuelve transparente donde va el vídeo.**

Encima del WebView taparía los botones, y haría falta dejar pasar los toques y rehacer los
controles en Swift y Kotlin (con i18n y accesibilidad por duplicado). Debajo, el WebView recibe
todos los toques y dibuja sus controles sobre el vídeo tal cual.

El WebView dice dónde va cada imagen con un **`VideoLayout`**: rectángulos en píxeles CSS medidos
con `getBoundingClientRect()` del hueco de cada vídeo, más si el propio va en espejo y su radio de
esquina. Lo manda al montar la pantalla de llamada, en cada `ResizeObserver` o giro, y al
arrastrar la miniatura propia (a lo sumo una vez por `requestAnimationFrame`). `remote: null` y
`local: null` esconden las vistas sin soltarlas.

**iOS (Swift)**:

- `attachVideo(remoteLayer, localLayer)`:
  - un `FtVideoView` (UIView) se inserta con `webView.superview.insertSubview(_, belowSubview:
    webView)`: wry añade el `WKWebView` como subvista de la vista raíz;
  - lleva dos subvistas anfitrionas: la remota a pantalla completa y la propia con
    `cornerRadius` y `masksToBounds`;
  - `Unmanaged<CALayer>.fromOpaque(ptr).takeUnretainedValue()` y `addSublayer`, en el hilo
    principal;
  - el WebView se vuelve transparente: `isOpaque = false`, `backgroundColor = .clear` y
    `scrollView.backgroundColor = .clear`;
  - todo se deshace en `detachVideo`.
- Colocación: `webView.convert(rect, to: videoView)`. Un píxel CSS es un punto de UIKit con el
  zoom a 1.
- La capa remota (`AVSampleBufferDisplayLayer`, que conserva la proporción por sí misma) se coloca
  con `bounds` y `position`, **nunca `frame`**. Con un cuarto de vuelta (`videoShape`, rotación de
  90 o 270) se intercambian ancho y alto. La capa propia (`AVCaptureVideoPreviewLayer`) ya sale en
  espejo con la cámara frontal.
- **Orden al terminar**: el núcleo llama a `detach_video` (bloquea hasta que Swift quitó las capas
  en el hilo principal) y **después** suelta los dispositivos. Contrato del motor: las capas son
  de los objetos Rust.
- Orientación: `UIDevice.orientationDidChangeNotification`, tras
  `beginGeneratingDeviceOrientationNotifications()`, manda el evento `orientation` con
  `UIDevice.current.orientation.rawValue`.

**Android (Kotlin + JNI)**:

- `attachVideo`:
  - un `FrameLayout` se añade en el índice 0 del padre del WebView (`android.R.id.content`: wry
    hace `setContentView(webView)`), es decir, detrás del WebView;
  - dentro van dos `SurfaceView`: la remota, y la propia con `setZOrderMediaOverlay(true)` para
    que quede sobre la remota y bajo la ventana;
  - el WebView pasa a `setBackgroundColor(Color.TRANSPARENT)`.
- Rectángulos: los píxeles CSS se multiplican por `resources.displayMetrics.density`. La vista
  remota se ajusta a la proporción de `videoShape` (una `SurfaceView` estira su contenido). La
  miniatura propia va con esquinas rectas en esta fase: una `SurfaceView` no recorta.
- **Superficie → Rust**:
  - `SurfaceHolder.Callback.surfaceCreated/Changed` llama a
    `FtVideoSurfaces.nativeSurface(slot, surface)`, y `surfaceDestroyed` a
    `nativeSurface(slot, null)` antes de volver;
  - en Rust (`src-tauri/src/video_surfaces.rs`, solo Android), el JNI hace
    `ANativeWindow_fromSurface` y llama a `ft_media::views::set_surface(slot, window)`, que
    toma su propia referencia; luego suelta la suya (`ANativeWindow_release`);
  - la biblioteca de la app ya está cargada (Tauri hace `System.loadLibrary`), así que el
    `external fun` resuelve;
  - regla de R8 para conservar `FtVideoSurfaces` y su método nativo.
- Orientación: la actividad gira (sin `screenOrientation`), así que en cada cambio de configuración
  se manda el evento `orientation` con `display.rotation` en grados, para
  `CameraSource::set_display_rotation`.

**API del puente** (Rust en `platform/src/lib.rs`, ya en el commit de contrato; Swift y Kotlin
implementan los comandos con estos nombres y argumentos):

| Rust (`Platform`)                                  | Comando nativo  | Argumentos JSON                                          | Respuesta         |
| -------------------------------------------------- | --------------- | -------------------------------------------------------- | ----------------- |
| `attach_video(remote_layer: usize, local_layer: usize)` | `attachVideo` | `{ "remoteLayer": u64, "localLayer": u64 }` (0 en Android) | —               |
| `video_layout(&VideoLayout)`                       | `videoLayout`   | `{ "remote": Rect\|null, "local": Rect\|null, "mirrorLocal": bool, "localRadius": f64 }`, `Rect = { x, y, width, height }` en px CSS | — |
| `video_shape(width: u32, height: u32, rotation: u16)` | `videoShape`  | `{ "width", "height", "rotation" }`                       | —                 |
| `detach_video()`                                   | `detachVideo`   | —                                                         | — (iOS: tras quitar las capas) |
| `call_video(on: bool)`                             | `callVideo`     | `{ "on": bool }`                                          | —                 |
| `request_camera() -> bool`                         | `requestCamera` | —                                                         | `{ "granted": bool }` |

Eventos nuevos por el canal de llamadas (`NativeCallEvent`), con su forma en la tubería:

| Evento                    | JSON                                        | Quién lo manda                                                |
| ------------------------- | ------------------------------------------- | ------------------------------------------------------------- |
| `Visible(bool)`           | `{"event":"visible","visible":true}`        | iOS: `didBecomeActive` / `didEnterBackground` y el valor inicial al registrarse. Android: `onResume` / `onPause` del plugin (donde ya se lleva `InCall.appVisible`). |
| `Orientation(i32)`        | `{"event":"orientation","orientation":3}`   | iOS: `UIDeviceOrientation.rawValue`; Android: grados de `display.rotation`. Solo con vídeo enganchado. |
| `VideoRequested`          | `{"event":"video"}`                         | iOS: la *user activity* de llamada de vídeo del botón «Vídeo» de CallKit. Android: la acción de cámara de la notificación en curso. |

JNI (Android): `package com.flickertalk.platform; object FtVideoSurfaces { @JvmStatic external fun
nativeSurface(slot: Int, surface: Surface?) }`, con símbolo
`Java_com_flickertalk_platform_FtVideoSurfaces_nativeSurface(env, class, slot: jint, surface:
jobject)`. `slot` vale 0 para la remota y 1 para la propia (`ft_media::ViewSlot::from_raw`).

### 4. Contrato de la interfaz

`CallPage` enseña **siempre** el botón de voz/vídeo (icono de cámara, `aria-pressed`), activo si
`available` y desactivado si la llamada no tiene línea de vídeo. Con la cámara propia encendida se
añaden «girar cámara» y el propio botón la apaga. «Cámara apagada» y «voz» son lo mismo para este
lado: no hay dos botones.

Con vídeo en la llamada (`camera || remote`), la pantalla pasa al diseño de vídeo: remoto a
pantalla completa y miniatura propia. Si el otro enciende su cámara y la nuestra está apagada, el
botón de cámara se resalta: es la invitación, no se enciende sola.

**Estado en núcleo y eventos.** Una instantánea, no los eventos sueltos `VideoStarted` /
`VideoStopped` / «remoto sí/no»: un WebView que llega tarde o un evento perdido no dejan la
pantalla mal.

- `CallUpdate::Video(ft_media::VideoState)` sale en cada cambio.
- En el WebView es el evento `ft://call` con `kind: "video"`:

```json
{ "contact": "ft_…", "call": "…", "kind": "video",
  "available": true, "camera": true, "paused": false, "facing": "front",
  "remote": true, "remotePaused": false }
```

(`shape` no va al WebView: es asunto de las vistas nativas.) `core_current_call` gana
`video: CallVideoView` con los mismos campos, para `restoreCall`.

**Comandos de Tauri** (los define el trabajo 2; el 4 los llama con estos nombres):

| Comando                                                   | Qué hace                                                                                  |
| --------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `core_call_start_native({ contact, routing, video })`     | Como hasta ahora, y ahora también de vídeo (`video: true` enciende la cámara al conectar). |
| `core_call_answer_native({ call, routing })`              | Sin cambios de firma; contesta también las de vídeo. Contestar una de vídeo desde la app enciende la cámara. |
| `core_call_set_video({ call, on }) -> CallVideoView`      | Enciende o apaga la cámara propia. Con `on` pide antes el permiso (`request_camera`); si se deniega, error `camera_denied`. |
| `core_call_switch_camera({ call }) -> CallVideoView`      | Cambia entre la frontal y la trasera.                                                      |
| `core_call_video_layout({ layout: VideoLayout \| null })` | Dónde van las imágenes. `null` = la pantalla de llamada no las enseña (se sale de ella): vistas escondidas y cámara retenida. |

`core_native_calls` sigue diciendo si las llamadas son nativas: en los teléfonos, ahora también
las de vídeo.

**API del núcleo (`ft-core`, trabajo 2)**:

```rust
impl Core {
    pub fn set_call_video(&self, platform: Option<ft_media::VideoPlatform>);       // al arrancar: ft_media::platform_video()
    pub async fn start_native_call(self: &Arc<Self>, contact: &str, routing: CallRouting, video: bool) -> Result<String>;
    pub async fn set_call_camera(&self, call: &str, on: bool) -> Result<VideoState>;
    pub async fn switch_call_camera(&self, call: &str) -> Result<VideoState>;
    pub async fn set_call_shown(&self, shown: bool) -> Result<()>;   // la pantalla de llamada enseña el vídeo
    pub async fn set_app_visible(&self, visible: bool) -> Result<()>; // NativeCallEvent::Visible
    pub async fn request_call_video(&self) -> Result<()>;             // NativeCallEvent::VideoRequested
    pub fn call_video(&self, call: &str) -> Option<VideoState>;
}
// Retenida = camera && !(app_visible && call_shown).
```

`answer_ringing_call` deja de dejar las de vídeo al WebView. Contestada así, con el teléfono
bloqueado, la cámara queda encendida pero retenida hasta que la app esté delante.

**Pegamento en `client.rs` (trabajo 2)** ante cada `CallUpdate::Video(state)`:

1. `platform.call_video(state.any())`.
2. Si hay dispositivos por primera vez: `platform.attach_video(layers…)`, con
   `ft_media::views::layers()` en iOS y 0 en Android.
3. `platform.video_shape(…)` cuando cambia `shape`.
4. Al acabar la llamada: `platform.detach_video()` **antes** de que el núcleo suelte los
   dispositivos.

`core_call_video_layout` pasa el `VideoLayout` a `platform.video_layout` y hace
`core.set_call_shown(layout.is_some())`. `NativeCallEvent::Orientation` va a
`ft_media::views::set_orientation`.

### 5. Permisos

- **Cámara al encenderla, no antes** (`§30`, como Signal). Lo hace `core_call_set_video`, y
  también `core_call_start_native` y `core_call_answer_native` cuando la llamada es de vídeo.
  - iOS: `AVCaptureDevice.requestAccess(for: .video)`. `NSCameraUsageDescription` ya está,
    traducido en `InfoPlist.strings`.
  - Android: permiso `CAMERA` en tiempo de ejecución con la API de permisos del plugin. `CAMERA`
    ya está en el manifiesto de la app.
  - Denegado: la cámara no se enciende, la llamada sigue de voz y el WebView lo dice con un icono
    y un texto i18n.
- **Contestar bloqueado sin permiso**: no se puede pedir en segundo plano, así que la cámara queda
  apagada; el usuario la enciende después desde la app.
- **Android, servicio en primer plano**:
  - `FtCallService` declara `phoneCall|microphone|camera`, y el manifiesto del puente
    `FOREGROUND_SERVICE_CAMERA`;
  - `callVideo(true)` con la cámara propia encendida vuelve a llamar a `startForeground` con
    `camera` añadido, y `callVideo(false)` lo quita; solo si `CAMERA` está concedido y la app está
    visible o viene de la acción de la notificación (la excepción documentada);
  - si falla, se sigue sin `camera`, como hoy con `microphone`;
  - **por qué, si la cámara se retiene en segundo plano**: evita que el sistema corte la cámara en
    las transiciones (el motor lo vería como cámara perdida y la reiniciaría), deja lista la fase
    PiP y es lo que hace Signal;
  - **pendiente de Ioan**: declarar en Play Console el tipo `camera`, junto a `phoneCall` y
    `microphone`.

### 6. CallKit y pantallas del sistema

- **iOS**:
  - `callVideo(on)` hace `reportCall(with: current, updated: update(caller: nombre, video: on))`,
    con `on = state.any()`: cualquiera de las dos cámaras, aunque esté retenida. Así, contestada
    bloqueada, al desbloquear se abre la app (DTS, hilo 798090). Signal usa solo la cámara propia;
    nosotros queremos que desbloquear lleve a la imagen del otro también.
  - La llamada saliente de vídeo pasa `video: true` a `callStartedOutgoing`
    (`CXStartCallAction.isVideo`).
  - **Botón «Vídeo» de CallKit**: abre la app. El puente atiende la *user activity*
    (`INStartCallIntent` con `callCapability == .videoCall`, o `INStartVideoCallIntent`) y manda
    `VideoRequested`, que el núcleo toma como «encender mi cámara» (con permiso). Declarar
    `NSUserActivityTypes` en el `Info.plist`.
  - **Riesgo**: la *user activity* la gestiona tao/Tauri (`AppDelegate`). Si no llega al plugin, el
    botón solo abre la app, y el usuario enciende la cámara en ella: aceptable. El trabajo 3 lo
    comprueba.
  - **Bloqueado**: el audio sigue. Nuestra cámara no puede correr (queda encendida y retenida, y
    el otro ve «cámara en pausa»). La imagen del otro llega pero no se ve hasta desbloquear.
- **Android**:
  - la notificación en curso se vuelve a publicar con `setIsVideo(state.any())`;
  - gana una acción de cámara (encender o apagar), un `PendingIntent` de actividad con
    `CALL_ACTION=video` que abre la app y manda `VideoRequested` (o la apaga);
  - la entrante usa `forIncomingCall(...).setIsVideo(video)`;
  - **a comprobar**: cuántas acciones extra enseña el sistema en una `CallStyle`;
  - Telecom autogestionado sigue pendiente; cuando llegue, la cámara se refleja con
    `requestCallType` (Core-Telecom ≥ 1.1).

### 7. Despliegue

- **iOS y Android**: `runsNatively(video)` pasa a ser `nativeVoice` (todas las llamadas nativas). El
  camino WebView (`getUserMedia` + `RTCPeerConnection`) queda solo para el escritorio y para una
  llamada restaurada de antes de actualizar.
- Los tests que hoy fijan «a una oferta de WebView con vídeo se contesta solo audio» (en
  `ft-media` y `ft-core`) pasan a fijar «se contesta audio y vídeo».
- **Escritorio**: sigue en el WebView con `media: 0` (sin cambio de modo). Fase 2, sin tocar el
  núcleo:
  1. `addTransceiver('video', { direction: 'sendrecv' })` siempre;
  2. `replaceTrack(track | null)` para encender o apagar, sin renegociar;
  3. `media: 1` en `core_call_start` y `core_call_answer`;
  4. los mismos `core_call_set_video` y `kind: "video"`.
- Dependencia: `ft-media` pasa de la rama `audio-engine` a **`main`** del motor (commit de
  contrato; `Cargo.lock` en `4f1b660`). Los tests de voz siguen verdes con ella.
- Riesgo de interoperabilidad: el motor solo habla H.264 CB y no está probado contra navegadores
  reales (README del motor). Hay que probarlo contra el WebView de Android y de iOS de la 1.0/1.2
  antes de publicar.

## Contratos exactos

### Protocolo (`ft-protocol`, en el commit de contrato)

```rust
pub const CALL_MEDIA_VERSION: u16 = 1;

CallOffer  { call: MessageId, sdp: String, video: bool, #[serde(default)] media: u16 },
CallAnswer { call: MessageId, sdp: String, #[serde(default)] media: u16 },
CallMedia  { call: MessageId, seq: u32, video: bool, paused: bool },
```

En CBOR (`type`/`data`, como todo `Body`): `{"type":"call_media","data":{"call":…,"seq":7,
"video":true,"paused":false}}`, rellenado a 160 bytes como todo paquete. El núcleo aún manda
`media: 0` (comportamiento sin cambios) e ignora `CallMedia`: el trabajo 2 los activa.

### `ft-media` (en el commit de contrato, con *stubs* que devuelven error)

```rust
pub use webrtc_engine::video::{Facing, VideoError, VideoSink, VideoSource};

pub struct VideoDevices { pub source: Box<dyn VideoSource>, pub sink: Box<dyn VideoSink> }
pub type VideoFactory = Arc<dyn Fn() -> Result<VideoDevices, VideoError> + Send + Sync>;
#[derive(Clone)] pub struct VideoPlatform { pub devices: VideoFactory }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VideoState {
    pub available: bool, pub camera: bool, pub paused: bool, pub facing: Facing,
    pub remote: bool, pub remote_paused: bool, pub shape: Option<RemoteShape>,
}
impl VideoState { pub fn sending(&self) -> bool; pub fn any(&self) -> bool }

pub struct Video;
impl Video {
    pub fn for_session(session: &MediaSession, platform: VideoPlatform) -> Self;
    pub async fn connected(&self) -> Result<()>;
    pub async fn set_camera(&self, on: bool) -> Result<VideoState>;
    pub async fn set_paused(&self, paused: bool) -> Result<VideoState>;
    pub async fn switch_camera(&self) -> Result<VideoState>;
    pub async fn set_remote(&self, video: bool, paused: bool) -> Result<VideoState>;
    pub fn state(&self) -> VideoState;
    pub fn changes(&self) -> watch::Receiver<VideoState>;
    pub async fn stop(&self);
}

pub fn platform_video() -> Option<VideoPlatform>;           // None en escritorio

pub mod views {
    pub enum ViewSlot { Remote, Local }                      // from_raw(0|1), as_raw
    pub unsafe fn set_surface(slot: ViewSlot, window: *mut c_void) -> Result<()>;   // Android
    pub struct Layers { pub remote: usize, pub local: usize }
    pub fn layers() -> Option<Layers>;                       // iOS
    pub fn set_orientation(raw: i32);
    pub struct RemoteShape { pub width: u32, pub height: u32, pub rotation: u16 }  // quarter_turn()
}

// feature `testing`
pub fn testing::fake_video() -> (VideoPlatform, VideoProbe);  // made(), camera(), display()
```

Reglas de `Video`:

- `available` pasa a `true` al conectar si las dos direcciones de vídeo quedaron negociadas.
- `set_camera(true)` sin `available` es un error.
- `set_remote` enciende la pantalla aunque nuestra cámara esté apagada.
- `stop` para `VideoCall` y suelta los dispositivos (después de que el puente quitó las vistas).

### Puente

Tabla de la sección 3. Rust, en el commit de contrato: `NativeCallEvent::{Visible, Orientation,
VideoRequested}`, `VideoRect`, `VideoLayout`, `attach_video`, `video_layout`, `video_shape`,
`detach_video`, `call_video` y `request_camera`, con tests de la forma en la tubería. En escritorio
no hacen nada, y `request_camera` devuelve `true`.

## Reparto: cuatro trabajos en paralelo

Todos parten del commit de contrato de `native-video`. Cada uno trabaja en su rama y su *worktree*
y solo toca sus ficheros. Los `CLAUDE.md` de cada carpeta se ponen al día en local (no se
versionan).

### Trabajo 1: `ft-media` (vídeo en Rust)

**Ficheros**: `crates/ft-media/**` (`video.rs`, `views.rs`, `session.rs`, `platform.rs`,
`testing.rs`, `lib.rs`, `Cargo.toml`).

- `MediaSession`:
  - `add_video_track` siempre y `on_track` por tipo;
  - `video_sender()` y `take_remote_video()` (para `RemoteVideo::pending`);
  - saber si la línea de vídeo quedó `sendrecv` (para `available`).
- `Video` según las reglas de arriba: fuente con compuerta, `VideoCall` que arranca y para con
  `any()`, `pause`/`resume`, `switch_camera`, `set_remote` y `shape`.
- `views`: registro de proceso.
  - Android: superficies guardadas con `ANativeWindow_acquire` y aplicadas con
    `set_surface`/`set_preview_surface` cuando existan los dispositivos.
  - iOS: capas publicadas por la fábrica.
  - Orientación a `set_device_orientation`/`set_display_rotation`.
- `platform_video()` para iOS y Android sobre `platform_source()`/`platform_sink()`, envueltos en
  compartidos.
- **Nombres que no cambian**: todos los del bloque «`ft-media`» de arriba.

### Trabajo 2: núcleo y pegamento Rust

**Ficheros**:

- `crates/ft-core/src/{native_calls.rs, calls.rs, lib.rs}`;
- `crates/ft-core/tests/native_calls.rs` y un nuevo `crates/ft-core/tests/native_video.rs`;
- `src-tauri/src/client.rs` y `src-tauri/src/lib.rs` (registro de comandos y
  `set_call_video(ft_media::platform_video())`).

**Tareas**:

- `media: CALL_MEDIA_VERSION` en las ofertas y respuestas nativas, y guardar el `media` del otro.
- `CallMedia`: envío con `seq` y reintento, recepción (gana el `seq` más alto) → `Video::set_remote`.
- La API del núcleo de la sección 4, `CallUpdate::Video` y `CurrentCall.video`.
- `answer_ringing_call` también para vídeo.
- La retención por `Visible` y `set_call_shown`.
- En `client.rs`:
  - los comandos `core_call_set_video`, `core_call_switch_camera` y `core_call_video_layout`, y
    el parámetro `video` en `core_call_start_native`;
  - el evento `kind: "video"`;
  - el pegamento al puente de la sección 4;
  - los `NativeCallEvent` nuevos.
- **Depende** del trabajo 1 para que sus tests de integración pasen. Mientras tanto escribe los
  tests contra `testing::fake_video()` y el contrato (rojos hasta que llegue el 1).

### Trabajo 3: puente nativo (Swift, Kotlin, JNI)

**Ficheros**:

- `src-tauri/platform/ios/**`;
- `src-tauri/platform/android/**` (Kotlin, tests, manifiesto del puente: `FOREGROUND_SERVICE_CAMERA`
  y `foregroundServiceType="phoneCall|microphone|camera"`; `ft_strings.xml` en los 21 idiomas
  para la acción de cámara de la notificación);
- el nuevo `src-tauri/src/video_surfaces.rs` (JNI, `#[cfg(target_os = "android")]`, con sus tests
  de la conversión de `slot`), más **una línea** `mod video_surfaces;` en `src-tauri/src/lib.rs`
  (aviso: el trabajo 2 también toca ese fichero; es un conflicto trivial);
- `src-tauri/gen/apple/flickertalk_iOS/Info.plist` (`NSUserActivityTypes`) y la regla de R8 en
  `gen/android`;
- `src-tauri/platform/src/lib.rs` solo si hay que corregir el contrato, y avisando.

**Tareas**:

- los comandos `attachVideo`, `videoLayout`, `videoShape`, `detachVideo`, `callVideo` y
  `requestCamera`;
- los eventos `visible`, `orientation` y `video`;
- CallKit `hasVideo` y la *user activity* del botón «Vídeo»;
- el servicio en primer plano con `camera`;
- `CallStyle.setIsVideo` y la acción de cámara.

**Nombres**: los de las tablas de la sección 3.

### Trabajo 4: WebView

**Ficheros**:

- `src/calls.ts` y `src/calls.test.ts`;
- `src/views/CallPage.vue` y `src/views/CallPage.test.ts`;
- `src/i18n/*.json` (los 21);
- el núcleo falso de los e2e (`e2e/`) y la barra de «volver a la llamada» si hace falta.

**Tareas**:

- `runsNatively` a todas las llamadas nativas en los teléfonos;
- el botón de voz/vídeo siempre visible, «girar cámara», la invitación cuando el otro enciende su
  cámara, el diseño de vídeo con huecos transparentes (clase en `html` mientras hay vídeo nativo,
  fondos de Ionic transparentes solo entonces), y la miniatura arrastrable;
- `core_call_video_layout` con `ResizeObserver` y `requestAnimationFrame` (y `null` al salir de la
  pantalla);
- el evento `kind: "video"` y `restoreCall` con `video`;
- el altavoz al pasar a haber vídeo (si estaba en el auricular);
- textos nuevos en los 21 idiomas: `calls.video`, `calls.switchCamera`, `calls.cameraPaused`,
  `calls.cameraDenied`, `calls.videoUnavailable`, `calls.turnOnCamera`.

El escritorio no cambia.

**Orden de fusión**: el contrato ya está. Luego 1, luego 2 (encima), y 3 y 4 en cualquier momento:
solo dependen de los nombres.

## Plan de pruebas

### Automáticas (sin red; `cargo test --workspace`, clippy, Vitest, Kotlin, Swift en el simulador)

- **`ft-protocol`** (hecho): viaje de ida y vuelta de `CallMedia` y de `media`; oferta y respuesta
  antiguas leídas como `media: 0`; una app anterior lee la oferta nueva e ignora `CallMedia`;
  campos futuros de `CallMedia` ignorados.
- **`ft-media`**, dos `MediaSession` reales en loopback con `fake_video()`:
  - la oferta lleva `m=video` `sendrecv` H.264 `42e01f`;
  - encender la cámara en un lado hace que el otro muestre fotogramas (`probe.display().shown`
    crece) **sin oferta nueva**;
  - apagarla para la cámara (`camera().running == false`) y deja de llegar vídeo;
  - la imagen del otro no abre nuestra cámara (compuerta);
  - `switch_camera` cambia `facing` con la cámara encendida o apagada;
  - `set_paused` para y vuelve sin tocar `camera`;
  - `available` es falso con una oferta de voz de WebView (sin `m=video`) y verdadero con la de
    vídeo;
  - una oferta de vídeo de WebView se contesta con vídeo `sendrecv`;
  - los dispositivos se hacen una sola vez por llamada (`made() == 1` tras encender y apagar
    varias veces);
  - `stop` los suelta.
- **`ft-core`** (`tests/native_video.rs`, dos teléfonos completos, router falso, WebRTC en
  loopback, audio y vídeo falsos):
  - llamada de voz → A enciende → B recibe `CallUpdate::Video { remote: true }` y fotogramas → B
    enciende → los dos ven → A apaga → B `remote: false` → vuelta a voz, **con la voz sonando todo
    el rato** (correlación > 0,7);
  - los dos encienden a la vez (sin *glare*, estado final coherente);
  - `CallMedia` repetido o tardío (`seq` menor) no cambia nada;
  - llamada de vídeo contestada por el SO con la app no visible: cámara encendida y retenida, el
    otro ve `remote_paused`, `set_app_visible(true)` la suelta;
  - otro con `media: 0` y voz: `set_call_camera` da error y `available` es falso;
  - otro con `media: 0` y vídeo: vídeo desde el principio y ningún `CallMedia` enviado;
  - colgar para vídeo y voz en los dos lados;
  - salir de la pantalla (`set_call_shown(false)`) retiene la cámara.
- **`src-tauri`**: la forma JSON de `kind: "video"` y de `CallVideoView`; la tabla del pegamento
  (qué comando del puente sale ante cada estado) como función pura con tests, como ya se hace
  con `native_screen` y `ringing`.
- **Puente**: los tests Rust de la tubería (hechos).
  - Kotlin: tipos del servicio con cámara (`callServiceTypes` con `camera`), paso de píxeles CSS
    a px, ajuste a la proporción, `FtVideoSurfaces` y el `slot`.
  - Swift (simulador): la cuenta de `bounds` y `position` con cuarto de vuelta, la conversión de
    rectángulos y el mapeo de la *user activity* a `VideoRequested`.
- **WebView**: Vitest del botón (visible siempre, desactivado sin `available`), de «girar», del
  evento `video`, del `layout` que se manda y de `null` al salir. E2E con el núcleo falso: pasar
  de voz a vídeo y volver.

### En dispositivos (a mano; el simulador de iOS no tiene cámara, así que el vídeo va en el iPhone)

Pares: iPhone 13 mini ↔ Samsung S20+, y Samsung ↔ Lenovo. Para cada par:

1. Voz → vídeo → voz desde cada lado, varias veces. La voz nunca se corta.
2. Llamada de vídeo desde el principio, y apagar y encender la cámara.
3. Los dos pulsan a la vez.
4. Girar la cámara; girar el teléfono (la orientación llega bien al otro).
5. iPhone bloqueado:
   - entra una llamada de vídeo, se contesta en la pantalla bloqueada: se oye y el otro ve «en
     pausa»;
   - se desbloquea y se abre la app: la cámara arranca sola;
   - el botón «Vídeo» de CallKit en una llamada de voz: abre la app y enciende la cámara (o, si
     la *user activity* no llega, al menos abre la app).
6. Mandar la app a segundo plano con vídeo: el otro ve «en pausa»; volver: sigue.
7. Android: la notificación en curso con `setIsVideo` y la acción de cámara con la app en segundo
   plano.
8. Permiso de cámara denegado: la llamada sigue de voz, con aviso.
9. Contra una app 1.0/1.2 de la tienda: su llamada de vídeo se ve en los dos sentidos (H.264 con
   su WebView); su llamada de voz tiene el botón desactivado.
10. Batería y temperatura en 10 minutos de vídeo; ruta TURN (`always`) con vídeo.

## Pendiente y fuera de este diseño

- PiP con cámara en iOS (`AVPictureInPictureVideoCallViewController`,
  `isMultitaskingCameraAccessEnabled`) y en Android.
- El vídeo en segundo plano en Android (hoy se retiene, como en iOS).
- Compartir pantalla, y la renegociación reservada arriba si hiciera falta.
- Esquinas redondeadas de la miniatura en Android (`SurfaceView` no recorta).
- El escritorio (fase 2, receta en la sección 7).
- Telecom autogestionado con `requestCallType`.
- Pruebas del motor contra navegadores reales (RTX, *pacing*, estimación de ancho de banda del
  emisor: pendientes del motor).
