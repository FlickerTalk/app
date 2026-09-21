# app/src/

Frontend de la app: **Vue 3 + Ionic** (`@ionic/vue`, `@ionic/vue-router`, Ionicons) con
TypeScript y Vite (`§83`). Es el mismo stack que el hub de ERPlora; los tests, con Vitest + Vue
Test Utils y Playwright.

Diseño aprobado el 2026-09-21 (`§84`). Estructura:

| Ruta                 | Contenido                                                                  |
| -------------------- | -------------------------------------------------------------------------- |
| `router.ts`          | pestañas `/tabs/{chats,calls,settings}` y conversación `/chat/:id`         |
| `views/`             | `TabsPage` (pestañas + rail), `ChatsPage`, `ChatPage`, `CallsPage`, `SettingsPage` |
| `components/`        | `NavRail`, `ChatThread`, `MessageBubble`, `Avatar`                          |
| `theme/`             | `variables.css` (tokens de Ember, Aurora y Mono, claro y oscuro), `base.css` |
| `theme.ts`           | color y apariencia elegidos en Ajustes                                      |
| `mock/chats.json`    | datos de ejemplo hasta que existan los comandos de `ft-core`                |

Cada componente tiene su test al lado (`*.test.ts`, Vitest + Vue Test Utils + happy-dom); los
tests stubean Ionic (`src/__tests__/setup.ts`). Comandos: `npm test`, `npm run typecheck`.

Pendiente: los textos están escritos directamente en inglés; falta pasarlos al catálogo i18n. Las
preferencias de tema viven en `localStorage` hasta que exista el almacén local de `ft-storage`.
Faltan las pantallas de llamada, añadir contacto, detalle de contacto y primera ejecución.

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
- Ajustes incluye el interruptor del buzón (`§19`), **activado por defecto**: si se desactiva,
  no se guarda nada del usuario en el servidor. También el color (Ember por defecto, Aurora o
  Mono) y la apariencia (oscuro por defecto, claro o sistema).
- En pantallas anchas el contenido se desplaza con `.ft-tabs-frame`, no sobre `ion-tabs`:
  `IonTabs` pone un `inset: 0` inline que pisaría cualquier regla CSS.
- Las llamadas de voz y vídeo (`§66`) necesitan su propia pantalla (componente por definir).
- **Estilo** (`§84`): bonito, minimalista y con un toque moderno. **Responsive**: aunque de
  momento solo se usa en la app móvil, la UI es web y se adapta al ancho; la barra de pestañas
  inferior pasa a ser un NavigationRail (barra lateral de iconos) en pantallas anchas, y la
  lista de chats y la conversación se ven lado a lado. Sin fuentes ni recursos externos.
