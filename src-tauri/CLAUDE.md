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
  `pushToken` y `requestNotifications`. Tests de Kotlin:
  `(cd gen/android && ./gradlew :tauri-plugin-ft-platform:testDebugUnitTest)`.
- Firebase: `gen/android/app/google-services.json` **no se versiona** (el repo es público); está
  en `infra/secrets/` y CI lo escribirá desde un secreto. Sin él la app compila, sin push.
- En el `AndroidManifest.xml` de la app, las copias de Android están desactivadas
  (`allowBackup=false` y `data_extraction_rules.xml` sin nada): el historial y las claves no salen
  del teléfono (`§61`).

La demo `greet` y `tauri-plugin-opener` vienen de la plantilla y se sustituirán en el PoC 0.

## Deuda de la plantilla frente al plan

- `app.security.csp` es `null` y `withGlobalTauri` es `true`: hay que endurecerlos antes de
  producción y antes de cargar plugins (`§55`, `§58`).

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
