# app/src/

Frontend de la app: **Vue 3 + Ionic** (`@ionic/vue`, `@ionic/vue-router`, Ionicons) con
TypeScript y Vite (`§83`). Es el mismo stack que el hub de ERPlora; los tests, con Vitest + Vue
Test Utils y Playwright.

Diseño aprobado el 2026-09-21 (`§84`). Estructura:

| Ruta                     | Contenido                                                                |
| ------------------------ | ------------------------------------------------------------------------ |
| `router.ts`              | `/welcome`, pestañas `/tabs/{chats,calls,settings}`, `/chat/:id`, `/add-contact`, `/contact/:id`, `/call/:id`, `/circle/:id`, `/circle/:id/info`, `/new-circle`, `/plugin/:id`, `/backup`; `onboardingGuard` manda la primera ejecución a `/welcome` |
| `views/`                 | `TabsPage` (pestañas + rail), `ChatsPage`, `ChatPage`, `CallsPage`, `SettingsPage`, `WelcomePage`, `AddContactPage`, `ContactPage`, `CallPage`, `BlockedPage`, `MovePage`, `PluginsPage`, `PlanPage`, `SessionPage`, `HoursPage`, `CirclePage`, `CircleInfoPage`, `NewCirclePage`, `PluginPage`, `BackupPage` |
| `components/`            | `NavRail`, `ChatThread`, `CircleThread`, `MessageBubble`, `Avatar`, `QrCode`, `ScannerOverlay`, `EmojiPicker`, `IncomingCall`, `CallBar`, `PluginSheet` |
| `theme/`                 | `variables.css` (tokens de Ember, Aurora y Mono, claro y oscuro), `base.css` |
| `theme.ts`               | color y apariencia elegidos en Ajustes                                    |
| `core.ts`                | puente con el núcleo Rust: almacén reactivo (`me`, `chats`, mensajes) alimentado por los comandos `core_*` y el evento `ft://changed` |
| `plugins.ts`             | herramientas instaladas: **una sola lista** (`installed` + `refreshPlugins()`) para toda la app, la URL del marco de cada plugin y lo único que el marco puede decir (`fromFrame`) |
| `preferences.ts`         | enrutado de llamadas y si ya se vio la bienvenida                         |
| `i18n.ts`, `i18n/*.json` | catálogo de textos; inglés (`en.json`) como fuente y 20 traducciones que se cargan según el idioma del teléfono (`§84`) |

Cada componente tiene su test al lado (`*.test.ts`, Vitest + Vue Test Utils + happy-dom); los
tests stubean Ionic e instalan el catálogo (`src/__tests__/setup.ts`). El puente de Tauri se
simula con `__tests__/tauri.ts` (el código real de `@tauri-apps/api` se ejecuta) y `seed()`
(`__tests__/seed.ts`) llena el almacén con `__tests__/chats.fixture.json`, que solo existe para los
tests. Comandos: `npm test`, `npm run typecheck`.

**E2E** (`e2e/`, Playwright, `npm run test:e2e`): la app real en Chromium contra el servidor de
Vite, con un núcleo falso en memoria (`e2e/fake-core.ts`) detrás de `window.__TAURI_INTERNALS__`.
Cubren solicitudes, sesiones (entrar ≠ crear, borrar), ficheros que esperan, permisos de plugins y
círculos (lista, crear, hilo, ajustes).
`PW_CHROMIUM=/ruta/a/chrome` usa un Chromium ya instalado. La app marca lo que los tests buscan
con `data-test` (`testIdAttribute` en `playwright.config.ts`).

Estado (2026-09-22, `§106` M3): los datos son reales. La identidad nace en el núcleo; la
bienvenida pide un nombre opcional (viaja en la Contact Card). «Añadir contacto» muestra el QR de
la tarjeta firmada, copia el enlace, escanea con la cámara (`@tauri-apps/plugin-barcode-scanner`,
solo en móvil) o acepta un enlace pegado. El hilo carga los mensajes, envía y marca como leído. El
buzón se activa en el núcleo. La cabecera del hilo dice «Direct» solo si hay un DataChannel abierto
con el contacto (`connected` de `core_conversations`; el núcleo avisa al abrirse o cerrarse).
Ficheros (M5): el «+» del compositor es un `ion-fab` que despliega dos botones (decisión de Ioan,
2026-09-23): **foto o vídeo** abre la hoja modal de fotos del sistema (`ACTION_PICK_IMAGES`, se ve el
chat detrás y se cierra deslizando) y **fichero** abre el selector de documentos
(`ACTION_OPEN_DOCUMENT`, a pantalla completa y sin cierre visible: solo el «atrás» del sistema).
Ambos van por `core_pick_files` (puente nativo); el `<input type="file">` del WebView queda solo
para escritorio, porque en Android saca al usuario de la app sin vuelta. El compositor es un `div`
dentro del `ion-footer`, no una `ion-toolbar`, porque esta recorta lo que se despliega por encima.
Las notas de voz van por `sendFile`, que copia el fichero a la app en trozos de 512 KiB en base64
(en Android el IPC de Tauri solo lleva JSON) y el núcleo lo ofrece. La burbuja muestra el progreso,
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

Estado (2026-09-23): herramientas, acciones del mensaje y plan.

- **Herramientas** (`§53`, issue app#3): el botón de apps de la cabecera del hilo abre una hoja con
  lo instalado y cada una se abre en su propia ventana (`PluginSheet`), con cabecera propia (✕ y el
  nombre, bajo `env(safe-area-inset-top)`) porque el marco del plugin ocupa toda la pantalla. Lo
  que un plugin propone entra en el compositor: lo envía el usuario, nunca el plugin. La tienda
  está en Ajustes → Plugins (`PluginsPage`): junta semillas y catálogo, enseña el peso de cada una
  y concede permisos uno a uno. **La lista es única** (`plugins.ts`): instalar o quitar en Ajustes
  se ve en el hilo sin salir de la conversación (probado en un Samsung real, 2026-09-23). El botón
  no está cuando no hay ninguna instalada.
- **Iconos del marco**: un plugin no trae imágenes ni fuentes; pide los iconos al núcleo
  (`./icon/<nombre>.svg`, los mismos Ionicons de la app) y el marco lleva
  `color-scheme: light dark` para que no salga blanco en modo oscuro.
- **Acciones del mensaje** (decisión de Ioan, 2026-09-23): una pulsación larga sobre una burbuja
  ofrece cuatro cosas —plegar (acordeón), reenviar a otra conversación, compartir con otra app
  (hoja del sistema) y borrar de este teléfono, que pregunta una vez porque es para siempre.
- **Burbujas de medios** (decisión de Ioan, 2026-09-23): una foto, un vídeo o una nota de voz no
  llevan tarjeta, nombre ni tamaño; la burbuja es el medio. Foto y vídeo llenan la burbuja sin marco,
  con la hora y el icono de guardar superpuestos; la nota de voz es solo su reproductor (play/pausa,
  progreso y duración; `chat.play`/`chat.pause`). Los demás ficheros conservan icono y nombre, sin
  tamaño. El estado sigue a la vista (`§84`): anillo con el porcentaje sobre la foto, porcentaje en la
  nota de voz y «Failed» cuando falla. La onda de la nota de voz es dibujada, no medida: 18 barras
  (de 3 px como mínimo, con 3 px de aire) cuya altura sale del id del mensaje, encendidas hasta
  donde se ha reproducido. El paseo que las genera tira hacia el 60 %, así que sube y baja suave y
  no se aplasta contra los topes: con más barras o sin ese freno se veía como estática, no como una
  onda (Ioan, 2026-09-23).
- **Plan** (`§40–47`): `PlanPage` (Ajustes → Plan) dice cuánto queda del año gratis, ofrece el euro
  y pregunta la edad (nunca la fecha de nacimiento). La compra todavía contesta «todavía no»:
  falta el producto en las tiendas.

Estado (2026-09-27, plugins fase 3 y la nube del usuario; `docs/drive.md`):

- **«Abrir con»** en la pulsación larga de una burbuja (`open-with-<id>`): los plugins cuyo
  manifiesto `opens` el tipo del fichero (o `text/plain` para un texto, con `messages`). La hoja
  recibe `file`, `reference` (el `ref` opaco del mensaje) y `live`; a un plugin con `drive`
  concedido se le da solo el nombre y el tipo del fichero, no los bytes. `PluginPage`
  (`/plugin/:id?reminder=`) abre un plugin solo, desde Ajustes → Plugins («Abrir») o desde un
  aviso tocado (`App.vue` lo enruta con `pendingReminder()`).
- `PluginSheet` contesta además `ft.record*`, `ft.remind*`, `ft.liveSend` (solo con `live`
  concedido y un contacto), `ft.openChat` (emite `openChat` y la página navega) y `ft.drive`
  (una pregunta con `op` y hasta dos textos; antes pregunta al núcleo si el plugin tiene `drive`;
  `upload` abre el selector desde la app, `keep` guarda por el `ref`, `send` baja el fichero y lo
  deja en el compositor con `propose` o lo envía con `auto`). Lo que dice el otro lado llega por
  `ft://plugin` y se reenvía al marco como `ft.live`.
- `PluginsPage` tiene un interruptor por permiso nuevo (`live`, `remind`, `drive`, `storage`
  grande) y enseña qué abre cada plugin.
- **Visor de documentos** (2026-09-27): tocar un fichero (`tapFile` en `ChatThread`) lo abre en
  el plugin que `views` su tipo (`viewerOf` en `plugins.ts`: tipo exacto; entre dos, el instalado
  más tarde), con los bytes y el `ref`; sin visor, o si los bytes no se pueden entregar (no está
  entero, pasa de 32 MB), va a otra app (`openFile`) como antes. `openIn(plugin, message)` es lo
  común entre el toque y «Abrir con», que con visor ofrece además **«Otra app»**
  (`open-elsewhere`, `chat.otherApp`). En iOS no hay plugins descargados: el toque va siempre a
  Quick Look.
- **Botón Atrás de Android** (2026-09-28, `back.ts`): lo que está abierto encima (plugin, apps,
  acciones de un mensaje, emoji) se cierra con Atrás, lo último primero (`closeOnBackWhile`). Solo
  se escucha el botón (`onBackButtonPress`, permisos `core:app:allow-register-listener` y
  `allow-remove-listener` en `capabilities/mobile.json`) mientras hay algo abierto: sin nada, Atrás
  hace lo de siempre. Antes, con un plugin abierto, sacaba del chat o, en la tableta, de la app.
  Una superposición nueva se registra con `closeOnBackWhile`.
- **Copia de seguridad** (`BackupPage`, Ajustes → Copia de seguridad): conectar Google Drive
  (login en el navegador del sistema, por el núcleo), crear el drive con la **frase de
  recuperación** que el usuario escribe dos veces (sugerir, copiar, compartir; 2026-09-28), abrir
  el drive de otro teléfono con la frase (intentos que quedan y bloqueo de 24 h tras cinco
  fallos, `triesLeft`/`retryAt`), cambiar la frase, hacer copia,
  restaurar (pregunta una vez; la app reinicia) y olvidar la nube (pregunta una vez). Escucha
  `ft://vault` y `ft://vault-progress`. Los textos están en `backup.*`, 21 idiomas. La frase solo
  se escribe aquí: un plugin no puede crear ni abrir el drive (`PluginSheet` rechaza `setup` y
  `unlock`).

## Reglas

- El frontend **no tiene secretos ni habla con el servidor**: clave privada, push token y claves
  de sesión solo existen en el core Rust (`§54`). Todo pasa por comandos Tauri hacia `ft-core`.
- **Estado del mensaje siempre visible y honesto** (`§84`): `○ pending`, `✓ sent`,
  `✓✓ delivered`, `✓✓ read`. Si el peer no conecta, se muestra que se espera al dispositivo.
  Nunca se finge una entrega.
- Sin presencia central (`§37`): `typing` solo existe mientras hay conexión P2P activa; no hay
  «last seen».
- **Iconos primero** (`§84`): emoji e iconos estándar en lugar de texto; texto solo si es
  imprescindible, desde el catálogo i18n. Cada icono lleva `aria-label`, también desde el catálogo.
- **Traducciones** (decisión 2026-09-23, `§84`): cada clave nueva de `en.json` se añade **en el
  mismo cambio** a los 20 `i18n/<idioma>.json` (el test de `i18n.test.ts` falla si falta una clave o
  cambia un `{marcador}`). Sin plurales dependientes del número: redacción neutra («Días: {days}»).
  Nada de `left`/`right` en CSS: propiedades lógicas (`inset-inline-start`, `padding-inline-end`,
  `text-align: start`) para que el árabe (RTL) se vea bien. Los textos de notificación de Android
  viven en `src-tauri/platform/android/src/main/res/values*/ft_strings.xml`, con los mismos idiomas.
- **Solicitudes (A5)**: `store.requests` y `session.requests` son los desconocidos que escribieron
  primero; `ChatsPage` los enseña aparte con su id corto, aceptar y rechazar. **Sesiones (A3)**:
  todo PIN abre su sesión o una nueva vacía (`openSession`, un solo gesto); `closeSession` deja
  que el núcleo borre la vacía y `removeSession` borra la que sea. Solo con los siete huecos
  ocupados queda en `store.sessions` una sesión `id: ""` que solo existe en pantalla: se ve igual
  (papelera incluida), no deja añadir a nadie y se va sin avisar al núcleo. **Ficheros (A4)**: estado `waiting` en la burbuja con tamaño y botón de descarga
  (`acceptFile`); el límite se elige en Ajustes (`setAutoDownload`). **Plugins (A2)**:
  `PluginSheet` recibe `sending`; con `propose`, `ft.send` acaba en un adjunto en el compositor
  (`attach` → `staged` en `ChatThread`) que envía el usuario.
- **Círculos** (2026-09-27): `store.circles` y `session.circles` (`Circle`, con `members`,
  `admin`, `adminsOnly`, `left` y `lastSender` para la fila). `ChatsPage` los lista con sus propias
  filas y un botón de «nuevo círculo» (solo si hay a quién meter); `NewCirclePage` pide nombre y
  contactos de la lista donde se crea; `CircleThread` enseña quién dijo cada cosa (`sender` en
  `MessageBubble`) y lo que pasó como una línea (`circle.events.*`), y no deja escribir si solo
  escriben los administradores o ya no se está dentro; `CircleInfoPage` lleva miembros, invitar,
  expulsar, administradores, «solo administradores escriben», salir y borrar, con las acciones
  irreversibles preguntando una vez en el sitio. Solo texto: sin adjuntos ni llamadas.
- `PluginSheet` es el **único** punto donde se monta un plugin (ver `app/crates/ft-plugins`): un
  iframe servido por el esquema `ftplugin://`, sin origen y con su propia CSP. El frontend no
  habla con el plugin más que por `postMessage`, y solo acepta de él lo que `fromFrame` reconoce;
  todo lo demás lo resuelve el núcleo tras comprobar lo concedido. Nunca se hace `invoke` desde
  dentro del marco.
- La lista de herramientas se lee de `plugins.ts`, **no** de un `ref` por componente: si una
  pantalla instala o quita, las demás tienen que verlo sin remontarse.
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

Estado (2026-09-28, llamadas de voz nativas):

- En iOS y Android (`core_native_calls`), una llamada de **voz** no usa `getUserMedia` ni
  `RTCPeerConnection`: `startCall` llama a `core_call_start_native({ contact, routing })`,
  `acceptCall` a `core_call_answer_native({ call, routing })`, `toggleMute` a
  `core_call_mute({ call, muted })` y colgar sigue siendo `core_call_end` (el núcleo para la voz).
  El estado llega de los eventos del núcleo: `answered` → «connecting», `connected` → en curso (el
  reloj), `muted` (también desde CallKit o la notificación) y `ended`. Las videollamadas y el
  escritorio no cambian.
- `startCalls()` pregunta `core_current_call()` y recupera la llamada que suena o va: el núcleo
  pudo oír la oferta, o CallKit contestarla, antes de que el WebView escuchara. Una llamada en
  curso abre la pantalla de llamada; `applyCallNotification` actúa sobre ese estado. Una llamada del
  WebView que sobrevivió a su WebView ya no tiene media: acaba como fallida.
- La ruta de llamadas se le dice al núcleo al arrancar y en cada cambio (`syncCallRouting` en
  `preferences.ts`): CallKit contesta sin WebView con esa copia.
- Sin textos nuevos: la pantalla de llamada (`CallPage`, `IncomingCall`) es la misma.

Estado (2026-09-29, llamadas con la app en pantalla; bugs vistos en el iPhone):

- Con la app delante, contestar desde el banner de CallKit daba voz pero nunca abría `CallPage`:
  no había forma de colgar. Ahora `calls.ts` abre la pantalla de la llamada (`showCall`, solo si
  no está ya en ella) cuando la llamada que se enseña pasa a `answered` o `connected`, venga de
  donde venga la respuesta. Y el núcleo avisa al WebView en el acto cuando CallKit contesta
  (`ft://call-action`): `applyCallNotification` lee `core_pending_call` sin esperar a un
  `visibilitychange` (antes una videollamada contestada en el banner con la app delante no se
  contestaba nunca).
- `applyCallNotification` solo actúa sobre una llamada que **suena**: un «decline» viejo (o el fin
  de CallKit de una llamada ya colgada) ya no cuelga la llamada en curso.
- `CallBar` (en `App.vue`): con una llamada en marcha (`calling`, `connecting` o `active`) y otra
  pantalla delante, una píldora arriba con el nombre y el reloj vuelve a la llamada
  (`calls.backToCall`, 21 idiomas) y un botón rojo cuelga (`calls.hangUp`).
