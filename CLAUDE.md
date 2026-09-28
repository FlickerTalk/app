# app/

Todo el **cliente** de FlickerTalk. Es un proyecto Tauri 2 generado con `create-tauri-app`
(plantilla `vanilla-ts`, npm) al que se suman el núcleo Rust y los paquetes TS.

| Carpeta       | Contenido                                                                    |
| ------------- | ---------------------------------------------------------------------------- |
| `src/`        | frontend: Vue 3 + Ionic + TypeScript + Vite (`§83`)                          |
| `src-tauri/`  | proyecto Rust de Tauri: comandos, capabilities, bridge nativo, `gen/android` |
| `crates/ft-*` | núcleo Rust con la lógica de negocio (`§82`); `ft-vault` es la nube del usuario (`docs/drive.md`); `ft-media`, la media nativa de las llamadas |
| `packages/`   | `ui` (TypeScript)                                                            |

Los plugins **no** viven aquí: cada uno tiene su repo (`FlickerTalk/plugin-images`, `plugin-pdf`,
`plugin-redact`, `plugin-sketch`, `plugin-markdown` y, desde el 2026-09-27, `plugin-notes`,
`plugin-board` y `plugin-drive`, que piden el núcleo 1.1.0, y `plugin-pdf-viewer`, que pide el
1.2.0). Aquí está el runtime que los instala y
ejecuta (`crates/ft-plugins`) y la API que el núcleo les expone; el contrato para terceros vive
en `plugin-sdk/` (MIT).

**Semillas** (`src-tauri/resources/plugins/`): la app lleva dentro los paquetes **pequeños**
(hoy los cinco, 17,5 KB en total), para que un teléfono sin red los tenga y para que en iOS la
primera versión no descargue nada (App Store 4.7). Se copian tal cual del catálogo construido:

```sh
for d in ../web/site/plugins/com.flickertalk.*; do cp $d/*.ftplugin src-tauri/resources/plugins/$(basename $d | sed 's/com.flickertalk.//').ftplugin; done
```

Un test impide que una semilla pase de 32 KB (128 KB entre todas): **lo pesado no viaja, se
descarga**. La lista de Ajustes junta semillas y catálogo y se queda con la versión más nueva de
las dos; en iOS solo enseña las semillas.

La plataforma objetivo del MVP es **Android e iOS** (`§85`); escritorio compila pero no se publica.

## Comandos (desde `app/`)

```sh
npm install                                        # registry público vía app/.npmrc
npm test                                           # tests (Vitest)
npm run test:e2e                                   # e2e (Playwright + Chromium, núcleo falso; PW_CHROMIUM=… para uno propio)
npm run typecheck                                  # vue-tsc
npm run build                                      # vue-tsc + vite build → dist/
npm run tauri dev                                  # escritorio en modo desarrollo
npm run tauri android dev                          # requiere ANDROID_HOME y NDK_HOME
npm run tauri ios dev                              # requiere Xcode + `tauri ios init`
cargo test --workspace                             # tests de Rust (sin red)
cargo test -p ft-webrtc -- --ignored               # red real: STUN; TURN con FT_TURN_URL/USERNAME/CREDENTIAL
cargo check --workspace                            # comprobar el lado Rust
```

## La app real en dos emuladores (`§106` M3)

```sh
npm run tauri android build -- --debug --apk --target aarch64
for s in emulator-5554 emulator-5556; do adb -s $s install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk; done
```

Los dos usan `api.flickertalk.com`. Emparejar sin cámara: en uno, «Añadir contacto» → escanear →
pegar el enlace de la tarjeta del otro (`core_card`). Para automatizar, se reenvía el DevTools de
la WebView (`adb forward tcp:9333 localabstract:webview_devtools_remote_<pid>`) y se conecta
Playwright por CDP; las capturas de CDP salen deformadas en la cabecera, las buenas son las de
`adb exec-out screencap -p`. El núcleo y la base de datos viven en el directorio de datos de la
app (`storage.key`, `flickertalk.db`): desinstalar borra la identidad.

## Release (producción)

```sh
npm run tauri android build -- --apk --target aarch64   # APK firmado, minificado (R8)
npm run tauri android build -- --aab                     # para Google Play
```

**Versión 1.2.0 (2026-09-27, sin publicar)**: plugins fase 3 (registros, avisos, canal en directo,
«abrir con»), la nube del usuario (copia de seguridad y «Mi drive», `docs/drive.md`), los tres
plugins nuevos, y el visor de documentos (`views` en el manifiesto, el toque en el chat abre el
PDF en `plugin-pdf-viewer`; en iOS, Quick Look). `version` de `tauri.conf.json` y `CORE_VERSION`
de `ft-core` van a la par.

**Estado (2026-09-28): push en iOS y llamadas de voz nativas.** Nada de esto está fusionado:

- **iOS** (rama `ios-push`, PR app#13): push por APNs (el router 0.3.0 ya lo sirve) y llamadas con
  CallKit a través de PushKit; tocar una notificación ya no cierra la app (el delegado corre en el
  hilo principal); el registro del push espera al token de VoIP y la app vuelve a dejar su token
  al router cada vez que vuelve a la pantalla; colgar siempre sale de la pantalla de llamada; y el
  núcleo arranca desde un plugin de Tauri (`ft-boot`) y no con la ventana, así que una app que
  PushKit lanza sin escena también se conecta. Detalle en `src-tauri/CLAUDE.md`.
- **Llamadas de voz nativas** (rama `native-calls`, encima de `ios-push`, sin subir; Plan `§66`,
  decisión del 2026-09-28): en iOS y Android la **voz** de una llamada va en Rust, sin WebView, con
  el crate `crates/ft-media` sobre `webrtc-engine` (repo público `FlickerTalk/webrtc-engine-rs`,
  AGPL-3.0, dependencia git). Motivo: la media de WKWebView corre en otro proceso, se silencia en
  segundo plano y no recibe el micrófono de CallKit, así que una llamada contestada con el iPhone
  bloqueado no tenía audio. La señalización SDP no cambia: un teléfono nativo habla con un par que
  aún usa el WebView. Android: audio nativo con el servicio en primer plano `phoneCall|microphone`
  y, con el router 0.3.1 (PR server#3, sin fusionar), el push FCM `t: call` hace sonar el teléfono
  con la app cerrada. Las **videollamadas y el escritorio siguen en el WebView** hasta que se
  integre el vídeo del motor (en curso en `webrtc-engine-rs`). **En pruebas** en dispositivos
  reales desde el 2026-09-28; falta declarar en Play Console los servicios en primer plano
  `phoneCall` y `microphone`. Detalle en `crates/ft-core/CLAUDE.md` (`native_calls.rs`),
  `crates/ft-media/CLAUDE.md`, `src-tauri/CLAUDE.md` y `src/CLAUDE.md`.

**Estado (2026-09-23): la 1.0.0 está en revisión en Google Play.** El AAB (29,5 MB; 8,45 MB de
descarga) se subió a mano —la primera subida lo exige— desde la cuenta de organización ERPlora,
con la ficha, las capturas y el «Data safety» de `infra/store/play/` y del runbook
`infra/runbooks/ficha-google-play.md`. A partir de aquí las sube CI. Lo que falta para cobrar (el
perfil de pagos y la suscripción de 1 €) está en `infra/TAREAS.md`. iOS ya tiene la cuenta de pago
de Apple, pero su publicación espera a que Apple libere el bundle id (abajo, «Entorno»).

Compilar para Android necesita el NDK **que hay instalado**: hoy
`~/Library/Android/sdk/ndk/27.1.12297006`. Con otro número, el build falla con «Android NDK
invalid» y, si se encadena un `adb install`, se instala el APK **viejo** sin avisar.

**CI/CD** (`.github/workflows/app.yml`, Plan `§105`): una sola rama, `main`, y tags de versión.

- Cada PR pasa typecheck, Vitest, clippy, los tests de Rust, un build de Android sin firmar y los
  tests de Kotlin.
- Cada push a `main` publica un APK firmado como prerelease **canary** de GitHub.
- Cada tag `vX.Y.Z` (igual a `version` de `tauri.conf.json`) publica un release con el APK y el
  AAB firmados y, si existe la service account, sube el AAB a **Google Play** (pista `internal`;
  producción siempre a mano en la consola, `§105`). La **primera** subida del AAB tiene que ser
  manual: lo exige Google. Hasta que estén el secreto `PLAY_SERVICE_ACCOUNT_JSON` y la variable
  `PLAY_PACKAGE_NAME`, ese job se salta solo.
- La clave de subida y el `google-services.json` solo existen en el entorno `release` de GitHub
  (`main` y tags `v*`). Los PR, incluidos los de forks, nunca los ven.

La firma usa la **clave de subida** de Google Play (Play App Signing guarda la de la app):
`gen/android/keystore.properties` (fuera de git) apunta a `infra/secrets/android-upload.jks`; en CI
irá por variables de entorno (`ANDROID_UPLOAD_KEYSTORE_FILE`, `ANDROID_UPLOAD_KEY_ALIAS`,
`ANDROID_UPLOAD_PASSWORD`). Sin ellas, el release sale sin firmar. Los proc-macros no se hacen
`strip` en release (`build-override`): con Xcode 27 la dylib quedaba rota.

## Entorno

- `app/.npmrc` fuerza el registry público de npm, para no depender de registries privados
  configurados de forma global en la máquina.
- Android necesita `ANDROID_HOME` y `NDK_HOME`. Si no están exportadas, pásalas al comando
  (`ANDROID_HOME=… NDK_HOME=… npm run tauri android …`), y `JAVA_HOME` si el shell no carga
  sdkman. Si Gradle dice «A problem occurred starting process 'command 'npm''», es un daemon de
  Gradle arrancado con otro entorno: `GRADLE_OPTS=-Dorg.gradle.daemon=false`.
- iOS: Xcode completo (`xcode-select -p` → `/Applications/Xcode.app/…`), CocoaPods y `xcodegen`
  (Homebrew). `src-tauri/gen/apple/` se versiona **sin** equipo de firma: compila e instala
  `APPLE_DEVELOPMENT_TEAM=<team> scripts/ios-build.sh [udid]`, que pone el equipo en el proyecto
  solo mientras compila (la exportación lo necesita ahí).
  El **manifiesto de privacidad** que pide la App Store está en
  `src-tauri/gen/apple/flickertalk_iOS/PrivacyInfo.xcprivacy` (copiado a la raíz del paquete por la
  fase *Resources*): no se recoge ningún dato y se declaran las APIs de motivo obligado que usan la
  app y sus librerías. **No regeneres el proyecto con `xcodegen`**: borra las descripciones de
  cámara y micrófono del `Info.plist` y las líneas `DEVELOPMENT_TEAM = ""` que necesita
  `scripts/ios-build.sh`.
  **Bundle id en iOS (2026-09-28)**: `com.flickertalk.app` sigue retenido por el antiguo equipo
  personal (gratuito) y el equipo de pago no puede registrarlo hasta que Apple lo libere (caso
  abierto con Apple). Mientras tanto, el iPhone se prueba con una build **`.dev`**
  (`com.flickertalk.app.dev`) firmada en el equipo de pago y **solo local**: ese identificador y el
  equipo nunca se versionan (el router acepta los dos bundles, `FT_APNS_TOPICS`). No se compila
  con el equipo personal: mantendría el identificador ocupado. El ID de equipo nunca va en este
  repo; se pasa por `APPLE_DEVELOPMENT_TEAM`.
  Un iPhone nuevo se registra una vez con `xcodebuild -allowProvisioningUpdates
  -allowProvisioningDeviceRegistration -destination id=<udid> …`. Los permisos de cámara y micrófono
  están en `src-tauri/Info.ios.plist`. El WebView de iOS no habla CDP: en el iPhone se prueba a mano.

## Reglas

- **Poca lógica de negocio fuera de `crates/`** (`§82`): `src/` y `src-tauri/` son capas finas
  sobre `ft-core`. Si algo se puede testear sin Tauri, va en un crate.
- Workspace Cargo en `app/Cargo.toml`, con miembros **explícitos**: cada crate nuevo se añade a la
  lista. Un glob `crates/*` fallaría con las carpetas que aún solo
  tienen `CLAUDE.md`. El perfil de release vive en la raíz del workspace.
- El identificador `com.flickertalk.app` (`tauri.conf.json`, paquete Android) es
  **definitivo**. Es un contrato con las stores: cambiarlo implica regenerar `src-tauri/gen/`, y
  tras la primera subida a Google Play ya no se puede.
- **Segundo plano** (`§19–20`): en Android, FCM despierta la app y el core Rust inicia la
  negociación, respetando las políticas de batería y foreground services. En iOS el silent push
  no está garantizado: la entrega fiable va por notificación visible + Notification Service
  Extension, que recoge el mensaje cifrado del buzón y lo descifra en local (≈ 30 s y 24 MB de
  límite). El push nunca lleva el mensaje.
- Los permisos del SO (contactos, notificaciones…) se piden **solo cuando hacen falta** (`§30`).
