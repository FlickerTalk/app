# app/

Todo el **cliente** de FlickerTalk. Es un proyecto Tauri 2 generado con `create-tauri-app`
(plantilla `vanilla-ts`, npm) al que se suman el núcleo Rust y los paquetes TS.

| Carpeta       | Contenido                                                                    |
| ------------- | ---------------------------------------------------------------------------- |
| `src/`        | frontend (TypeScript + Vite; pasará a Vue 3 + Ionic, `§83`)                  |
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
npm run build                                      # tsc + vite build → dist/
npm run tauri dev                                  # escritorio en modo desarrollo
npm run tauri android dev                          # requiere ANDROID_HOME y NDK_HOME
npm run tauri ios dev                              # requiere Xcode + `tauri ios init`
cargo check --manifest-path src-tauri/Cargo.toml   # comprobar el lado Rust
```

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
- Todavía no hay workspace Cargo: `src-tauri/` es un proyecto Cargo independiente. Al crear el
  primer crate de `crates/` hay que montar el workspace (`src-tauri` + `crates/*`).
- El identificador `com.flickertalk.flickertalk` (`tauri.conf.json`, paquete Android) es
  **provisional**. Es un contrato con las stores: se fija el definitivo antes de la primera
  subida, y cambiarlo implica regenerar `src-tauri/gen/`.
- **Segundo plano** (`§19–20`): en Android, FCM despierta la app y el core Rust inicia la
  negociación, respetando las políticas de batería y foreground services. En iOS el silent push
  no está garantizado: la entrega fiable va por notificación visible + Notification Service
  Extension, que recoge el mensaje cifrado del buzón y lo descifra en local (≈ 30 s y 24 MB de
  límite). El push nunca lleva el mensaje.
- Los permisos del SO (contactos, notificaciones…) se piden **solo cuando hacen falta** (`§30`).
