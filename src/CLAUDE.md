# app/src/

Frontend de la app: **Vue 3 + Ionic** (`@ionic/vue`, `@ionic/vue-router`, Ionicons) con
TypeScript y Vite (`§83`). Es el mismo stack que el hub de ERPlora; los tests, con Vitest + Vue
Test Utils y Playwright.

Estado actual: es la plantilla `vanilla-ts` de `create-tauri-app` (`main.ts` con la demo
`greet`, `styles.css`, `assets/`). Se sustituye al construir la UI; **Vue e Ionic aún no
están instalados**.

Pantallas y componentes previstos: app, lista de chats, chat, mensaje, composer, contacto,
ajustes, llamada y el host de plugins (`<ft-plugin-host>`). Los componentes táctiles salen de
Ionic; los propios del chat se hacen en Vue encima de ellos.

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
- `<ft-plugin-host>` es el único punto donde se montan plugins (ver `app/crates/ft-plugins`). Los
  plugins son web components: Vue tiene que tratarlos como elementos personalizados
  (`compilerOptions.isCustomElement`), no como componentes suyos.
- `<ft-settings>` incluye el interruptor del buzón (`§19`), **activado por defecto**: si se
  desactiva, no se guarda nada del usuario en el servidor.
- Las llamadas de voz y vídeo (`§66`) necesitan su propia pantalla (componente por definir).
