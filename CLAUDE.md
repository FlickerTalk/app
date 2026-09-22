# app/

Todo el **cliente** de FlickerTalk. Es un proyecto Tauri 2 generado con `create-tauri-app`
(plantilla `vanilla-ts`, npm) al que se suman el núcleo Rust y los paquetes TS.

| Carpeta       | Contenido                                                                    |
| ------------- | ---------------------------------------------------------------------------- |
| `src/`        | frontend: Vue 3 + Ionic + TypeScript + Vite (`§83`)                          |
| `src-tauri/`  | proyecto Rust de Tauri: comandos, capabilities, bridge nativo, `gen/android` |
| `crates/ft-*` | núcleo Rust con la lógica de negocio (`§82`)                                 |
| `packages/`   | `ui` (TypeScript)                                                            |

Los plugins **no** viven aquí: cada uno tiene su repo. La app solo contiene el runtime que los
instala y ejecuta (`crates/ft-plugins`, con un modo desarrollador para probarlos en local); el
SDK que define su contrato vive en su propio repo (`plugin-sdk/`, MIT).

La plataforma objetivo del MVP es **Android e iOS** (`§85`); escritorio compila pero no se publica.

## Comandos (desde `app/`)

```sh
npm install                                        # registry público vía app/.npmrc
npm test                                           # tests (Vitest)
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

**CI/CD** (`.github/workflows/app.yml`, Plan `§105`): una sola rama, `main`, y tags de versión.

- Cada PR pasa typecheck, Vitest, clippy, los tests de Rust, un build de Android sin firmar y los
  tests de Kotlin.
- Cada push a `main` publica un APK firmado como prerelease **canary** de GitHub.
- Cada tag `vX.Y.Z` (igual a `version` de `tauri.conf.json`) publica un release con el APK y el
  AAB firmados. La subida a Google Play aún no está automatizada; la primera es manual.
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
  (`ANDROID_HOME=… NDK_HOME=… npm run tauri android …`).
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
