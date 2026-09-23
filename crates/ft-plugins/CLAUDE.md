# app/crates/ft-plugins

Runtime de plugins: manifest, instalación, verificación y permisos (`§48–58`). Es la **fase 6**
(`§95`) y **no entra en el MVP** (`§86`).

## Formato

Paquete `.ftplugin`: `module.json`, `dist/index.js`, `dist/style.css`, `assets/`, `signature`.
`module.json` lleva `id`, `name`, `version`, `minCoreVersion` y `components` (`§49`).

## Reglas

- Solo plugins web (HTML/CSS/JS/Web Components). **Nunca** código nativo dinámico (`§48`).
- Instalación: descarga → BLAKE3 → verificar firma Ed25519 → validar manifest → instalar →
  registrar el Web Component. **Nada se ejecuta antes de verificar la firma** (`§50`).
- Capa propia `PluginCapability`, independiente de Tauri:
  `plugin → Plugin API → permission check → Rust Core` (`§58`). Permisos granulares con
  consentimiento explícito; nunca «acceso a todo» al instalar (`§53`).
- Un plugin jamás recibe clave privada, push token, Keychain/Keystore ni clave maestra (`§54`).
- Red desactivada por defecto y bloqueada por CSP; si se permite, lista de dominios +
  consentimiento (`§55`).
- Dos clases (`§52`): extensión empaquetada (puede usar APIs Core autorizadas) y plugin web
  descargado (sandbox limitado; en iOS solo renderers, temas, presentación de mensajes,
  transformaciones locales y comandos sobre datos dados explícitamente — App Store 4.7).
- Una capacidad nativa nueva = nueva versión del Core + `minCoreVersion`, nunca código en el
  plugin (`§51`).
- **Modo desarrollador**: carga un plugin desde una carpeta local sin firmar, para que los autores
  lo prueben en la app real. Solo con activación explícita del usuario y un aviso visible; nunca
  en el flujo normal de instalación, que exige firma (`§50`).

## Estado (2026-09-22, issue app#3)

Primer trozo del runtime, todo en seco y con tests:

- **Paquete `.ftplugin`**: un zip con `module.json`, `dist/`, `assets/` y `signature`. Lo que se
  firma es el BLAKE3 de cada fichero encadenado con su ruta, así que cambiar, añadir o quitar un
  byte rompe la firma. `open()` verifica la firma **antes** de leer nada como manifest, y rechaza
  manifests inválidos (id que no es un nombre, versión que no lo es, componente sin guion) y
  cualquier ruta que se salga de la carpeta del plugin.
- **Confianza (decisión 2026-09-22, cierra el pendiente de `§50`):** firma **el catálogo**, con una
  clave que viaja en la app. El autor puede firmar además, pero la confianza viene del catálogo,
  que así puede revocar. Un plugin nunca se acepta por venir firmado por su autor.
- **Catálogo** (`§56`): `index.json` estático y firmado; `catalogue_entries()` no lo lee si la
  firma no es del catálogo, y `download()` exige que el paquete sea exactamente el que el índice
  listaba (hash, id y versión), no solo «algo firmado».
- **Instalación**: `install()` escribe en `<dir>/<id>`, `installed()` lista los manifests y
  `remove()` borra. Nada se escribe fuera de esa carpeta.

## Estado (2026-09-23, issue app#4)

El núcleo es un **puente**, no un contenedor: **ninguna herramienta viaja dentro de la app**.

- **Catálogo servido**: `https://flickertalk.com/plugins/index.json` + `index.json.sig` + los
  paquetes. Lo construye y firma `ftcatalogue` (bin de este crate) desde los repos de los plugins;
  la clave privada nunca sale de `infra/secrets/plugin-catalogue.key`. La app lo lee con
  `Core::catalogue()` y solo instala con `Core::add_plugin()`, que exige que el paquete sea byte a
  byte el que el índice listaba y que la URL sea del propio catálogo.
- **Qué se empaqueta**: solo `module.json` y `dist/**`. Los tests, la licencia y las herramientas
  del repo del plugin no corren en el teléfono y no se firman.
- **La Plugin API** (lo único que un plugin puede hacer, `app/src-tauri/src/plugins.rs` la
  inyecta en el marco antes de cargar el plugin): `onOpen`, `pickFile`, `send`, `say`, `save`,
  `print`, `fetch`, `store`, `close`. Cada llamada la resuelve el core después de comprobar lo
  concedido; `fetch` solo alcanza los hosts concedidos (la CSP es el segundo cerrojo) y `store` es
  la memoria del plugin, porque su marco no tiene origen y el navegador no le da ninguna.
- **Permiso nuevo**: `print`. Se pide en el manifiesto y se concede aparte, como los demás.
- El contrato para terceros vive en `FlickerTalk/plugin-sdk` (tipos + esquema del manifiesto).

Falta: el modo desarrollador (cargar una carpeta sin firmar) y la revocación desde el catálogo.
