# app/src-tauri/

Proyecto Rust de Tauri 2 (generado por `create-tauri-app`): arranque, comandos expuestos a la
WebView, capabilities/permissions y el **platform bridge** (`§5`).

## Estructura generada

- `src/lib.rs` — `run()` con `#[cfg_attr(mobile, tauri::mobile_entry_point)]`: aquí va el código
  (en móvil la app se compila como librería). `src/main.rs` solo llama a
  `flickertalk_lib::run()` para escritorio: **no se toca**.
- `tauri.conf.json` — configuración (identificador, build, ventanas, seguridad, bundle).
- `capabilities/default.json` — permisos de la ventana `main`.
- `gen/android/` — proyecto Android Studio generado por `tauri android init`; se versiona (la
  plantilla solo ignora `gen/schemas`). `gen/apple/` aún no existe (falta Xcode).
- `icons/`, `build.rs` — plantilla.
- `platform/` — plugin de Tauri propio (`tauri-plugin-ft-platform`), el **puente nativo**: lo que
  Android solo deja hacer a Kotlin. Hoy abre un fichero de la app en otro visor (su propio
  `FtFileProvider`, limitado a la carpeta `files/`) y lo copia a Descargas (MediaStore). La WebView no
  lo llama: solo el Rust de la app (`client.rs`). También lleva el push (M4): `FtMessagingService`
  recibe el aviso de FCM y, si la app no está en pantalla, muestra una notificación sin contenido;
  `pushToken` y `requestNotifications`. Y la **clave de almacenamiento** (`§94`): en Android se
  sella con una clave AES del Keystore que nunca sale de él; en iOS (`platform/ios`, Swift) vive en
  el Keychain, solo en este dispositivo. `storage.key.sealed` guarda la forma sellada; una clave en
  claro de antes se migra sola, y si el almacén no la abre es un error (nunca se crea otra: se
  perdería la identidad). Tests de Kotlin:
  `(cd gen/android && ./gradlew :tauri-plugin-ft-platform:testDebugUnitTest)`.
- Firebase: `gen/android/app/google-services.json` **no se versiona** (el repo es público); está
  en `infra/secrets/` y CI lo escribirá desde un secreto. Sin él la app compila, sin push.
- En el `AndroidManifest.xml` de la app, las copias de Android están desactivadas
  (`allowBackup=false` y `data_extraction_rules.xml` sin nada): el historial y las claves no salen
  del teléfono (`§61`).

## Seguridad del WebView (2026-09-22)

- **CSP** en `tauri.conf.json`: solo recursos propios, estilos en línea (Ionic y Vue los usan),
  imágenes y audio del protocolo `asset` (limitado a la carpeta `files/`) y la IPC de Tauri. Nada
  externo, sin `eval`, sin frames. Comprobada recorriendo el bundle de producción bajo la misma
  cabecera en Chromium.
- `withGlobalTauri: false`: el WebView no tiene `window.__TAURI__`.
- **Permisos** (`capabilities/`): del núcleo solo eventos y la versión; `opener` para `mailto:`
  (reportes, `§36`) y para abrir los enlaces `http(s)` de un mensaje en el navegador; en móvil, el
  escáner. Todo lo demás pasa por los comandos `core_*`.
- **Rutas que nombra la WebView** (M1, 2026-09-27): `core_read_picked` y `core_send_picked` solo
  aceptan ficheros dentro de `uploads/` (lo que deja el selector nativo), `files/uploads/` y
  `files/outgoing/` (lo que hace un plugin), canonicalizados (`picked_path`). En Android
  `app_data_dir()` es `dataDir`, y el selector escribe en `filesDir/uploads` = `files/uploads`.
- **Plugins** (A2): `core_plugin_made` recibe el id del plugin y decide por lo concedido: `auto`
  envía, `propose` devuelve un `PickedView` para el compositor, nada rechaza.
- **Círculos** (2026-09-27): `core_circles`, `core_circle`, `core_circle_create/invite/remove/
  set_admin/rename/admins_only/leave/forget/send/messages/mark_read`; `CircleView` lleva los
  miembros con el nombre que este teléfono usa para cada uno (el propio si es un contacto elegido,
  el de su tarjeta si no) y `SessionView.circles` los de una sesión. El evento `ft://changed`
  lleva `circle` cuando cambia una conversación de círculo.
- El PoC 0 ya no está en la app (pantalla, comandos ni dependencia); `crates/ft-poc` sigue como
  herramienta de desarrollo.
- **Plugins, fase 3** (2026-09-27): `core_plugin_record_*` (registros con cuota), `core_plugin_ref`
  y `core_plugin_open_chat` (camino de vuelta al mensaje, opaco), `core_remind_*` y
  `core_pending_reminder` (avisos locales; `sync_reminders` le pasa al puente nativo la lista
  entera al arrancar y con cada cambio), `core_plugin_live_send` (canal en directo, solo por
  conexión directa; lo que llega sale por `ft://plugin`), `core_plugins_opening` y
  `core_read_message_file` («abrir con»). El marco (`plugins.rs`, `frame_js`) expone
  `ft.records`, `ft.remind`, `ft.live`, `ft.openChat` y `ft.drive`.
- **La nube del usuario** (2026-09-27, `docs/drive.md`): `core_vault_status/connect/setup/unlock/
  disconnect`, `core_vault_list/mkdir/rename/move/remove/upload/upload_message/retry/
  cancel_pending/download/open/save/send`, `core_vault_backup/backup_info/restore` y
  `core_plugin_may_use_drive`. `core_vault_connect` abre el login con `platform.authorize` (Custom
  Tabs / `ASWebAuthenticationSession`) a través de `PlatformAuthorizer`; los tokens y la clave del
  drive se quedan en el núcleo, sellados. `core_vault_restore` deja la copia en la carpeta de
  mudanza y reinicia la app: `apply_move` la cambia al arrancar, como tras una mudanza. Eventos
  `ft://vault` y `ft://vault-progress`. `picked_path` admite también `files/drive/` (lo bajado del
  drive se puede enviar). El esquema de redirección de Google va en el `AndroidManifest` del
  puente como `${googleRedirectScheme}`, que `build.gradle.kts` rellena desde
  `FT_GOOGLE_CLIENT_ID` (el mismo que lee el núcleo con `option_env!`).
- **Puente nativo** (2026-09-27): el Kotlin compila y sus 33 tests pasan (los de avisos necesitan
  `org.json` de verdad en `testImplementation`: `android.jar` solo trae stubs). Probado en el
  Lenovo: el aviso llega al `AlarmManager` (`RTC_WAKEUP`, aproximado sin permiso de alarma exacta),
  salta con la app en segundo plano y, al tocarlo, la app abre el plugin (`App.vue` vuelve a
  preguntar por `pendingReminder` cada vez que la app pasa a visible). **Sin probar**: el Swift
  (no hay Xcode aquí), `authorize` contra Google (falta el cliente OAuth), avisos tras reinicio y
  en Doze.
- **Visor de documentos en iOS** (2026-09-27): `openFile(path, mime)` en `PlatformPlugin.swift`
  presenta un `QLPreviewController` (Quick Look: PDF, Office, Pages, texto e imágenes, dentro de
  la app; su botón de compartir manda el fichero a otra app) sobre `manager.viewController`;
  rechaza si el fichero no está o Quick Look no puede con él (`previewable`, con test). En Rust no
  cambia nada: `Platform::open_file` ya llamaba a `openFile` en móvil.

- **Push y llamadas en iOS** (2026-09-28): el iPhone registra en el router `apns` con
  `pasarela:bundle:token-hex[:token-voip-hex]` (`pushToken` en `PlatformPlugin.swift`). Las
  respuestas de iOS se añaden al delegado de Tauri y la pasarela se deduce del perfil. El router
  manda una clave (`FT_PUSH_WAKE`, `FT_INCOMING_CALL`) que el iPhone traduce con los
  `Localizable.strings` de `gen/apple/flickertalk_iOS/<idioma>.lproj`, los mismos textos que
  `ft_strings.xml` de Android. Los permisos de cámara y micrófono se traducen en
  `InfoPlist.strings`. Llamadas: PushKit (registrado al cargar el plugin) informa a CallKit en el
  acto; `startRinging` pone el nombre o informa si la app no está en pantalla; contestar o colgar
  en CallKit queda en `pendingCall`, como en Android. **Limitación** (hasta las llamadas nativas,
  abajo): contestar desde la pantalla bloqueada pide abrir la app para hablar, porque el audio va
  por el WebRTC del WebView.
  `UIBackgroundModes`: `audio`, `remote-notification` y `voip`. Entitlement `aps-environment`.
- **Puente de llamadas nativas** (2026-09-28, rama `native-calls-bridge`): el audio de la llamada
  pasa del WebView a Rust (`webrtc-engine`), así que el puente habla con el núcleo **sin WebView**.
  Contrato (`platform/src/lib.rs`, en escritorio no hace nada):
  - `listen_calls(handler)` con `NativeCallEvent { Answer, End, Mute(bool), AudioActivated,
    AudioDeactivated }`. El canal es un `tauri::ipc::Channel` creado en Rust: Tauri lo registra en
    su tabla global de canales (`plugin/mobile.rs`, `CHANNELS`) y viaja a Swift/Kotlin como
    `__CHANNEL__:<id>` en el comando `registerCallEvents`; su `send` vuelve por
    `send_channel_data` (JNI / puntero C) a esa tabla, sin pasar por ninguna ventana. En la
    tubería van como `{"event":"mute","muted":true}`. Lo que llega antes del registro espera en
    una cola nativa (16 como mucho; una llamada nueva olvida lo de la anterior) y sale en orden al
    registrarse. El handler corre en un hilo nativo propio (una cola serie en iOS, un executor en
    Android), nunca en el principal: puede volver a llamar al puente, pero no debe bloquear.
  - `call_started_outgoing(name, video)`, `call_connected()`, `call_ended()` y
    `request_microphone() -> bool` (se pide explícito antes de llamar o contestar; el WebView lo
    pedía con `getUserMedia`). `start_ringing`/`stop_ringing` siguen.
  - **Los comandos del puente no se llaman desde el hilo principal**: resuelven desde él y
    `run_mobile_plugin` bloquea hasta la respuesta.
- **iOS (CallKit)**: CallKit es dueño de la llamada y de la sesión de audio en los dos sentidos.
  Saliente: `CXStartCallAction` por `CXCallController`; en la acción se configura la sesión
  (`.playAndRecord`, `.voiceChat`, Bluetooth HFP y A2DP) y **no se activa**: lo hace el sistema,
  y `didActivate`/`didDeactivate` mandan `AudioActivated`/`AudioDeactivated`. `call_connected`
  informa de la saliente como conectada; una entrante contestada en la pantalla de la app (sonó
  allí, CallKit no la conocía) **se une a CallKit** en `call_connected` como llamada iniciada y
  conectada al momento, para tener sesión de audio: **el núcleo debe llamar a `call_connected`
  también en las entrantes**. Contestar en CallKit manda `Answer` y sigue dejando `pendingCall`;
  colgar manda `End`; silenciar, `Mute`. `stopRinging` ya no termina una llamada contestada ni
  una saliente. `call_ended` la cierra siempre (`remoteEnded` si llegó a hablarse, `unanswered`
  si no).
- **Android**: el router manda `{"t":"call","s":"N"}` (TTL 45 s) para una llamada. Con la app
  cerrada, `FtMessagingService` muestra la notificación de llamada entrante con el texto genérico,
  con las reglas de `wake` (sesión oculta cerrada → nada; app en pantalla → nada, la hace sonar el
  núcleo) y, fuera del horario semanal, **sin sonido** (como una llamada que suena en la app).
  Suena el **sistema**, no nuestro `Ringtone`: canal propio `ft.call.ringing` con el tono de
  llamada, `FLAG_INSISTENT` y `setTimeoutAfter` con lo que quede de los 45 s desde `sentTime`. Un
  proceso despertado por FCM puede congelarse mucho antes; el canal respeta solo el modo del
  timbre. Contestar o rechazar abre la app con `CALL_ACTION` como antes y corta el timbre.
  Durante la llamada (`call_started_outgoing`, `pendingCall` = `answer` y `call_connected`, que
  es idempotente): `MODE_IN_COMMUNICATION`, foco de audio de voz y el servicio en primer plano
  `FtCallService` (`phoneCall|microphone`; el de micrófono solo si `RECORD_AUDIO` está concedido
  y se vuelve a promocionar al conectar) con notificación en curso: colgar manda `End` y silenciar
  `Mute` por el canal. `call_ended` para el servicio, suelta el foco y deja el modo como estaba.
  Manifiesto del puente: `RECORD_AUDIO`, `MODIFY_AUDIO_SETTINGS`, `FOREGROUND_SERVICE`,
  `FOREGROUND_SERVICE_MICROPHONE`, `FOREGROUND_SERVICE_PHONE_CALL`, `MANAGE_OWN_CALLS` (lo exige
  el tipo `phoneCall`), el servicio y `FtCallActionReceiver`. Textos nuevos en los 21
  `ft_strings.xml`. **Pendiente**: Play pide declarar los servicios en primer plano de tipo
  `phoneCall` y `microphone` en la consola; nada de esto se ha probado aún en un teléfono.
- **Probar iOS**: los tests Swift del puente corren en el simulador (`xcodebuild test -scheme
  tauri-plugin-ft-platform -destination 'platform=iOS Simulator,name=iPhone 17'` desde
  `platform/ios`). El chat y las llamadas se prueban en el iPhone, porque el simulador no tiene
  WebRTC.

## Reglas

- **Pegamento, no lógica**: cada comando Tauri delega en `ft-core`. Validación, estado y reglas
  de negocio van en los crates.
- **Superficie mínima** (`§58`): expón solo los comandos que la UI necesita y restríngelos con
  capabilities/scopes. Los plugins **nunca** hacen `invoke` de comandos arbitrarios: pasan por la
  Plugin API de `ft-plugins` con su propio chequeo de permisos.
- Clave privada, push token y clave maestra **nunca** cruzan a la WebView (`§54`).
- **Kotlin/Swift solo donde el SO obliga** (`§5`): FCM/APNs, agenda de contactos, Google Play
  Billing/StoreKit 2, llamadas (PushKit + CallKit en iOS; notificación de llamada entrante en
  Android, `§66`), cámara y micrófono, y lo que imponga el SO (p. ej. señales de edad, `§43`). No son lenguajes de
  aplicación: el bridge obtiene el dato nativo y lo pasa al core.
