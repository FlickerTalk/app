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
| `core.ts`                | puente con el núcleo Rust: almacén reactivo (`me`, `chats`, mensajes) alimentado por los comandos `core_*` y el evento `ft://changed` |
| `preferences.ts`         | enrutado de llamadas y si ya se vio la bienvenida                         |
| `i18n.ts`, `i18n/en.json`| catálogo de textos; inglés como fuente, sin traducciones en la fase 1     |

Cada componente tiene su test al lado (`*.test.ts`, Vitest + Vue Test Utils + happy-dom); los
tests stubean Ionic e instalan el catálogo (`src/__tests__/setup.ts`). El puente de Tauri se
simula con `__tests__/tauri.ts` (el código real de `@tauri-apps/api` se ejecuta) y `seed()`
(`__tests__/seed.ts`) llena el almacén con `__tests__/chats.fixture.json`, que solo existe para los
tests. Comandos: `npm test`, `npm run typecheck`.

Estado (2026-09-22, `§106` M3): los datos son reales. La identidad nace en el núcleo; la
bienvenida pide un nombre opcional (viaja en la Contact Card). «Añadir contacto» muestra el QR de
la tarjeta firmada, copia el enlace, escanea con la cámara (`@tauri-apps/plugin-barcode-scanner`,
solo en móvil) o acepta un enlace pegado. El hilo carga los mensajes, envía y marca como leído. El
buzón se activa en el núcleo. La cabecera del hilo dice «Direct» solo si hay un DataChannel abierto
con el contacto (`connected` de `core_conversations`; el núcleo avisa al abrirse o cerrarse).
Ficheros (M5): el botón de adjuntar abre el selector del sistema (`<input type="file">`, que el
WebView de Android admite); `sendFile` copia el fichero a la app en trozos de 512 KiB en base64 (en
Android el IPC de Tauri solo lleva JSON) y el núcleo lo ofrece. La burbuja muestra el progreso,
«Paused» sin conexión directa, «Failed» si el hash no coincide y las imágenes con el protocolo
`asset` (limitado a `$APPDATA/files`, donde viven también las subidas). Tocar un fichero lo abre en
otra app y el botón de descarga lo copia a Descargas (puente nativo de `src-tauri/platform`); los de
otros, solo cuando han llegado enteros. Llamadas (M6, `calls.ts`): `getUserMedia` + `RTCPeerConnection` del WebView con el STUN y el
TURN del router (`core_call_ice`), filtrados según el enrutado de Ajustes; las descripciones van
enteras, sin trickle, por el núcleo. `IncomingCall` avisa encima de cualquier pantalla,
`CallPage` muestra la llamada (el vídeo del otro entero, sin recortar, y solo cuando llega) y
salir de ella cuelga; `CallsPage` es el historial del núcleo. En el emulador la cámara es
sintética, el micrófono no capta sin «host audio input» y QEMU puede colgarse con vídeo: las
llamadas se prueban en dispositivos reales. Pendiente: el resto del MVP (M7).

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
  sistema). El buzón vive en el núcleo (que se lo comunica a los contactos); las demás
  preferencias en `preferences.ts` y `theme.ts`, solo en el dispositivo.
- En pantallas anchas el contenido se desplaza con `.ft-tabs-frame`, no sobre `ion-tabs`:
  `IonTabs` pone un `inset: 0` inline que pisaría cualquier regla CSS.
- Las llamadas de voz y vídeo (`§66`) necesitan su propia pantalla (componente por definir).
- **Estilo** (`§84`): bonito, minimalista y con un toque moderno. **Responsive**: aunque de
  momento solo se usa en la app móvil, la UI es web y se adapta al ancho; la barra de pestañas
  inferior pasa a ser un NavigationRail (barra lateral de iconos) en pantallas anchas, y la
  lista de chats y la conversación se ven lado a lado. Sin fuentes ni recursos externos.
