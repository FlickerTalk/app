# app/src/

Frontend de la app: **Lit + Web Components + TypeScript** sobre Vite (`§83`). Nada de React,
React Native ni Angular.

Estado actual: es la plantilla `vanilla-ts` de `create-tauri-app` (`main.ts` con la demo
`greet`, `styles.css`, `assets/`). Se sustituye al construir la UI; **Lit aún no está
instalado**.

Componentes previstos: `<ft-app>`, `<ft-chat-list>`, `<ft-chat>`, `<ft-message>`,
`<ft-composer>`, `<ft-contact>`, `<ft-settings>`, `<ft-plugin-host>`.

## Reglas

- El frontend **no tiene secretos ni habla con el servidor**: clave privada, push token y claves
  de sesión solo existen en el core Rust (`§54`). Todo pasa por comandos Tauri hacia `ft-core`.
- **Estado del mensaje siempre visible y honesto** (`§84`): `○ pending`, `✓ sent`,
  `✓✓ delivered`, `✓✓ read`. Si el peer no conecta, se muestra que se espera al dispositivo.
  Nunca se finge una entrega.
- Sin presencia central (`§37`): `typing` solo existe mientras hay conexión P2P activa; no hay
  «last seen».
- **Iconos primero** (`§84`): emoji e iconos estándar en lugar de texto; texto solo si es
  imprescindible, en inglés y desde el catálogo i18n. Sin traducciones en la fase 1. Cada icono
  lleva `aria-label` en inglés.
- `<ft-plugin-host>` es el único punto donde se montan plugins (ver `app/crates/ft-plugins`).
- `<ft-settings>` incluye el interruptor del buzón (`§19`), **activado por defecto**: si se
  desactiva, no se guarda nada del usuario en el servidor.
- Las llamadas de voz y vídeo (`§66`) necesitan su propia pantalla (componente por definir).
