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

## PoC 0 en el emulador (`§87`, fases B1 y B2)

La pantalla del PoC usa por defecto el relay del clúster, `wss://api.flickertalk.com`, que entrega
nuestro STUN y un usuario TURN temporal (`turn.flickertalk.com`) en su bienvenida.

```sh
VITE_POC=1 npm run tauri android build -- --debug --apk --target aarch64
adb -s emulator-5554 install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
cargo run -p ft-poc --bin poc-peer -- --relay wss://api.flickertalk.com --room demo --role callee  # par del Mac
```

En la app: Ajustes → PoC 0, sala, rol (Call/Answer) y, si se quiere, «Always relay (TURN only)» →
Connect → Send hello. Con dos emuladores, uno llama y el otro responde. `VITE_POC=1` muestra la
entrada del PoC en builds que no son de desarrollo (un APK lo es).

Relay local, sin TURN: `(cd ../server && cargo run -p ft-router)` y, en la app, `ws://10.0.2.2:8787`
(el emulador llega al Mac por `10.0.2.2`).

## Entorno

- `app/.npmrc` fuerza el registry público de npm, para no depender de registries privados
  configurados de forma global en la máquina.
- Android necesita `ANDROID_HOME` y `NDK_HOME`. Si no están exportadas, pásalas al comando
  (`ANDROID_HOME=… NDK_HOME=… npm run tauri android …`).
- iOS pendiente: `npm run tauri ios init` necesita Xcode completo, así que
  `src-tauri/gen/apple/` aún no existe.

## Reglas

- **Poca lógica de negocio fuera de `crates/`** (`§82`): `src/` y `src-tauri/` son capas finas
  sobre `ft-core`. Si algo se puede testear sin Tauri, va en un crate.
- Workspace Cargo en `app/Cargo.toml`, con miembros **explícitos** (`src-tauri`, `crates/ft-webrtc`):
  cada crate nuevo se añade a la lista. Un glob `crates/*` fallaría con las carpetas que aún solo
  tienen `CLAUDE.md`. El perfil de release vive en la raíz del workspace.
- El identificador `com.flickertalk.flickertalk` (`tauri.conf.json`, paquete Android) es
  **provisional**. Es un contrato con las stores: se fija el definitivo antes de la primera
  subida, y cambiarlo implica regenerar `src-tauri/gen/`.
- **Segundo plano** (`§19–20`): en Android, FCM despierta la app y el core Rust inicia la
  negociación, respetando las políticas de batería y foreground services. En iOS el silent push
  no está garantizado: la entrega fiable va por notificación visible + Notification Service
  Extension, que recoge el mensaje cifrado del buzón y lo descifra en local (≈ 30 s y 24 MB de
  límite). El push nunca lleva el mensaje.
- Los permisos del SO (contactos, notificaciones…) se piden **solo cuando hacen falta** (`§30`).
