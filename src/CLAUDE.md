# app/src/

Frontend de la app: **Vue 3 + Ionic** (`@ionic/vue`, `@ionic/vue-router`, Ionicons) con
TypeScript y Vite (`§83`). Es el mismo stack que el hub de ERPlora; los tests, con Vitest + Vue
Test Utils y Playwright.

Diseño aprobado el 2026-09-21 (`§84`). Estructura:

| Ruta                     | Contenido                                                                |
| ------------------------ | ------------------------------------------------------------------------ |
| `router.ts`              | `/welcome`, pestañas `/tabs/{chats,calls,settings}`, `/chat/:id`, `/add-contact`, `/contact/:id`, `/call/:id`; `onboardingGuard` manda la primera ejecución a `/welcome` |
| `views/`                 | `TabsPage` (pestañas + rail), `ChatsPage`, `ChatPage`, `CallsPage`, `SettingsPage`, `WelcomePage`, `AddContactPage`, `ContactPage`, `CallPage` |
| `components/`            | `NavRail`, `ChatThread`, `MessageBubble`, `Avatar`, `QrCode`             |
| `theme/`                 | `variables.css` (tokens de Ember, Aurora y Mono, claro y oscuro), `base.css` |
| `theme.ts`               | color y apariencia elegidos en Ajustes                                    |
| `preferences.ts`         | buzón, enrutado de llamadas y si ya se vio la bienvenida                  |
| `i18n.ts`, `i18n/en.json`| catálogo de textos; inglés como fuente, sin traducciones en la fase 1     |
| `mock/chats.json`        | datos de ejemplo hasta que existan los comandos de `ft-core`              |

Cada componente tiene su test al lado (`*.test.ts`, Vitest + Vue Test Utils + happy-dom); los
tests stubean Ionic e instalan el catálogo (`src/__tests__/setup.ts`). Comandos: `npm test`,
`npm run typecheck`.

Pendiente: sustituir los datos de ejemplo por los comandos de `ft-core`; las preferencias viven en
`localStorage` hasta que exista el almacén local de `ft-storage`; el escaneo de QR es todavía un
marco de cámara falso y los estados de llamada son fijos.

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
  no se guarda nada del usuario en el servidor. También el enrutado de llamadas (`§17`: solo
  directa, relay cuando haga falta —por defecto— o siempre relay, que oculta la IP al contacto),
  el color (Ember por defecto, Aurora o Mono) y la apariencia (oscuro por defecto, claro o
  sistema). Todas estas preferencias viven en `preferences.ts` y `theme.ts`, solo en el
  dispositivo.
- En pantallas anchas el contenido se desplaza con `.ft-tabs-frame`, no sobre `ion-tabs`:
  `IonTabs` pone un `inset: 0` inline que pisaría cualquier regla CSS.
- Las llamadas de voz y vídeo (`§66`) necesitan su propia pantalla (componente por definir).
- **Estilo** (`§84`): bonito, minimalista y con un toque moderno. **Responsive**: aunque de
  momento solo se usa en la app móvil, la UI es web y se adapta al ancho; la barra de pestañas
  inferior pasa a ser un NavigationRail (barra lateral de iconos) en pantallas anchas, y la
  lista de chats y la conversación se ven lado a lado. Sin fuentes ni recursos externos.
