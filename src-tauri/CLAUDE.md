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
- **Puente nativo, sin compilar aquí** (no hay SDK de Android ni Xcode en el entorno de esta
  entrega): `authorize`, `setReminders`/`pendingReminder`, `ReminderReceiver`, `BootReceiver` y
  `AuthRedirectActivity` (Kotlin) y sus equivalentes en Swift tienen tests unitarios escritos pero
  **hay que compilarlos y probarlos en dispositivo** (`docs/drive.md`).

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
