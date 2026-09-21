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
