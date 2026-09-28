# app/crates/ft-core

Orquestador del cliente (`§82`) y fachada que consume la app Tauri. Coordina identidad,
almacenamiento, protocolo, WebRTC, push, contactos, billing y plugins.

Funciones base (`§85`): texto, ficheros, llamada de voz y videollamada, 1 a 1. Desde el
2026-09-27, también **círculos**: grupos pequeños y cerrados, solo de texto por ahora.

## Responsabilidades

- **Envío de texto** (`§18–19`): persistir en `ft-storage` (`pending` + `pending_outbox`) → si
  hay DataChannel, enviar por `ft-webrtc` → con el ACK, `delivered`. Sin conexión: `WAKE` vía
  `ft-push` → signaling → conexión → envío. Si P2P no conecta **y** emisor y destinatario tienen
  el buzón activado: cifrar E2EE (`ft-crypto`) y depositar en el buzón vía `ft-push` → `sent`;
  si no, el mensaje espera en el outbox. Reintentos según `retry_state`/`next_attempt` (`§26`).
- **Ficheros** (`§62–64`): solo P2P, en chunks con hash, reanudables; el fichero sigue en el
  emisor hasta completar la transferencia. Nunca van al buzón.
- **Llamadas** (`§66`): voz y vídeo 1 a 1 por WebRTC; tiempo real, nunca por el buzón. La
  llamada entrante llega por el bridge (CallKit/PushKit en iOS, notificación de llamada en
  Android).
- **Recepción** (`§27`): mensajes por P2P o recogidos del buzón; deduplicar por `message_id`, y
  si ya existe, **ACK sin reinsertar**. Tras guardar un blob del buzón, ACK al router para que lo
  borre.
- **Bloqueo** (`§35`): rechazar signaling, mensajes, ficheros, llamadas, blobs del buzón y
  conexiones de `blocked_devices`.
- **Acceso** (`§42`): tras el primer año gratuito, aplicar el resultado de `ft-billing` (menor →
  gratis; adulto → entitlement o paywall).

## Reglas

- Un mensaje no sale del `pending_outbox` hasta recibir `delivered` (`§19`, `§26`).
- Al buzón solo llega texto cifrado a nivel de mensaje, nunca texto en claro (`§28`), y solo si
  los dos extremos lo permiten.
- Sin UI ni código de plataforma: expone una API que la app llama.

## Estado (2026-09-22, `§106` M1)

`Core` empareja por Contact Card, envía y recibe texto cifrado con acuses de entrega y lectura,
usa el buzón solo si los dos lo tienen activado y reintenta desde `pending_outbox` (5 s, 10 s…
hasta 5 min sin conexión; 30 s esperando el acuse tras un envío directo; 10 min si está en el
buzón). La red es el trait `Transport` (envío directo y buzón): los tests usan una red en memoria
con dos dispositivos completos (`tests/conversation.rs`); la implementación real sobre WebRTC y el
router es el hito M2. Hasta que un contacto responde (`introduced`), nuestra tarjeta viaja antes
de cada reintento.

## Estado (2026-09-22, `§106` M2)

`net::Network` es el `Transport` real: DataChannels WebRTC (`ft-webrtc`) por contacto, abiertos con
una oferta cifrada con Olm y enviada por el router (`ft-push`), que lleva la Contact Card en un
primer contacto; si el contacto no está conectado o el canal no abre en 12 s, el core usa el
buzón. Los eventos del router (bienvenida con STUN/TURN, señales, aviso de correo) se atienden en
segundo plano para que una respuesta nunca espere detrás de otro trabajo. `Relay` abstrae el
router (fake en `tests/network.rs`, con WebRTC real en loopback). `tests/live.rs` (ignorado) prueba
dos núcleos contra `api.flickertalk.com`: emparejan, envían y confirman en ~9 s.

## Estado (2026-09-22, `§106` M3)

`online::start` une `Core`, `RouterClient` y `Network` para la app: registra el dispositivo,
escucha el router y reintenta el `pending_outbox` cada 5 s. La app (`src-tauri/src/client.rs`)
lo arranca en segundo plano y ningún comando espera a la red. Probado en dos emuladores: emparejar
por enlace, texto en los dos sentidos y acuses `delivered` y `read`.

## Estado (2026-09-22, `§106` M5)

Ficheros (`files.rs`): la oferta (nombre, tamaño, tipo, BLAKE3) va por el outbox como un texto,
pero **solo por conexión directa**; nada de un fichero llega al buzón. El receptor pide los trozos
(48 KiB, `FILE_CHUNK`) en ventanas de 16 (`FileRequest`), los escribe en su sitio, comprueba el
hash al final y avisa (`FileDone`); si no coincide, borra los bytes y lo marca como fallido. Una
transferencia parada 15 s se vuelve a pedir desde el primer trozo que falta (`resume_files`, desde
el bucle de reintentos de `online`). La app fija la carpeta con `set_files_dir`. Probado en memoria,
con WebRTC real en loopback y entre los dos emuladores (imagen y 3 MB, directo).

## Estado (2026-09-22, `§106` M6)

Llamadas (`calls.rs`): la media es el WebRTC del WebView; el núcleo lleva la oferta, la respuesta y
el final (`CallOffer`, `CallAnswer`, `CallEnd`), cifrados con Olm y solo por conexión directa (el
DataChannel se abre bajo demanda por el router), nunca por el buzón. Una llamada a la vez: otra
oferta recibe «busy» y queda como perdida; una llamada que suena más de 60 s sin respuesta deja de
bloquear. El historial (`calls` en SQLite) solo vive en el teléfono; al arrancar se cierran las
llamadas que quedaron abiertas. `Event::Call` avisa a la UI. Al abrirse una conexión se reanudan
en el acto las transferencias de ficheros de ese contacto (`resume_files_from`).

## Estado (2026-09-22, `§106` M4)

Push: la app deja su token de FCM al router (`RouterClient::set_push`, desde `core_enable_push` tras
el onboarding y en cada arranque). El router despierta a un dispositivo no conectado cuando le llega
una señal o un correo; en Android sale una notificación sin contenido que abre la app. Una llamada
sigue intentándolo hasta 40 s (`CALL_REACH`) mientras el otro teléfono despierta. Probado en una
Lenovo real con la app cerrada: notificación a los ~10 s y mensaje entregado al abrirla
(`tests/live.rs`, `writes_to_a_real_phone`). iOS (APNs) espera a la cuenta de pago de Apple.

## Estado (2026-09-27, paquete de endurecimiento; `tests/hardening.rs`)

- **A1, sobre sellado**: lo que va al router (`send_mailbox` y las señales de `net.rs`) se mete en
  un `Envelope` sellado para la clave de sobre del destinatario (`wrap_for`); `receive` y
  `on_signal` lo abren (`unwrap`). Un contacto cuya tarjeta no trae clave de sobre (app antigua)
  recibe los bytes tal cual. Al arrancar por primera vez con clave de sobre, `card_stale` hace que
  `online::start` reparta la tarjeta nueva (`reintroduce`).
- **M9, relleno**: `Packet::encode` rellena a bloques de 160 bytes.
- **A2, plugins**: `plugin_sending`, `plugin_may_propose` y `plugin_send_file` (solo con `auto`).
- **A3, sesiones** (decisión 2026-09-27: todo PIN es válido): `open_session` abre la sesión del
  PIN o crea una vacía, sin retardo ni «fallos»; `None` solo si los siete huecos están ocupados.
  `close_session` borra la sesión si está vacía (sin contactos, solicitudes ni círculos) y devuelve si se
  fue; al arrancar se borran las vacías que quedaran. `remove_session` borra todo. Una sesión que
  se va se lleva su enlace (`forget_session` rota la capability del hueco; la app vuelve a
  registrar los ocho hashes), y un primer contacto con un `via` que ya no es nuestro no llega a
  nadie (`session_via`).
- **A4, ficheros**: `auto_download_limit` (10 MB por defecto); por encima, `waiting` hasta
  `accept_file`; una oferta mayor que `MAX_FILE_SIZE` (2 GB) se rechaza con `FileFailed`.
- **A5, solicitudes y enlace**: un desconocido que escribe primero queda `accepted = 0`
  (`requests`, `accept_contact`, `decline_contact`; escribirle o escanearlo acepta). Sin aceptar:
  sin ruido, sin trozos de fichero, llamadas «busy». `renew_link` rota la capability (principal o
  de una sesión) y reparte la tarjeta.
- **M5/M6**: `received_at` manda en orden y retención; el barrido respeta el `pending_outbox`.
- **B6**: `clean_name` doma el nombre que trae una tarjeta (40 caracteres, sin control ni bidi).

## Estado (2026-09-27, círculos; `circles.rs`, `tests/circles.rs`)

Un círculo es su tarjeta firmada (`ft-circles`). Todo lo dicho en él va a cada miembro por el canal
Olm 1 a 1 que este teléfono ya tiene con él, directo o por su buzón, igual que un mensaje a un
contacto: el router ve n sobres sellados y nada más (probado: ni el id del círculo, ni el emisor,
ni el nombre aparecen en los blobs). El mismo `Packet`, con el mismo id, llega a todos; el acuse de
cada miembro limpia su entrada de `circle_outbox` (`circle_receipt`), y el mensaje pasa a
`delivered` cuando no queda nadie por alcanzar.

- `create_circle`, `invite_to_circle`, `remove_from_circle`, `rename_circle`,
  `set_circle_admins_only`, `set_circle_admin` (solo administradores; el último no puede dejar de
  serlo), `leave_circle` (avisa a todos; lo dicho se queda en solo lectura), `forget_circle`,
  `send_circle_text`, `mark_circle_read` (no viaja ningún acuse de lectura en círculos).
  `circle_writable` dice antes de enviar si este teléfono puede escribir (está dentro y, con «solo
  administradores», lo es): la app envía en segundo plano y un error del envío se perdería. Una
  salida emite `CirclesChanged` y `CircleMessagesChanged`, para que el hilo abierto enseñe «X salió».
- **Miembros como contactos de círculo** (`contacts.via_circle`): al adoptar una tarjeta, cada
  miembro desconocido se guarda con `accepted = 0` y `via_circle = 1` y se le abre un canal Olm
  desde su Contact Card. No sale en la lista ni en Solicitudes hasta que escribe 1 a 1 por su
  cuenta (entonces es una solicitud, A5) o el usuario lo escanea. Si deja de estar en ningún
  círculo y no ha escrito, se olvida (`prune_circle_contact`).
- **Recepción**: `CircleCard` solo de su firmante, y solo si sigue la cadena de la tarjeta que se
  tiene; un círculo nuevo solo de un contacto aceptado (la tarjeta de un desconocido espera sin
  acuse hasta que se le acepte, y llega con el siguiente reintento). `CircleMessage` de quien no
  es miembro según mi copia se ignora sin acuse (un reintento lo trae cuando ya tenga la tarjeta);
  en un círculo del que salí, se contesta `Received` para que dejen de reintentar. `CircleLeave`
  quita al miembro de la copia local; la siguiente revisión de un administrador lo consolida.
- **Sesiones ocultas**: un círculo creado en una sesión es de sus contactos y vive con ella
  (`circles.session`); borrar la sesión se lo lleva. En una sesión cerrada no hace ruido.
- La tarjeta y el «me voy» viajan por el mismo `circle_outbox` como mensajes de tipo `card` y
  `leave` que nunca se enseñan; un reintento de `card` manda siempre la tarjeta **actual**.
- Cada revisión que firma este teléfono (`revise`) lleva la Contact Card **que este teléfono
  tiene** de cada miembro, no la que traía la tarjeta, y la suya propia tal como es ahora: si un
  miembro renovó su enlace (A5), quien entra después le alcanza. Si un administrador tuviera una
  tarjeta más vieja que la de la revisión anterior, la del miembro se corrige sola con el
  `ContactCard` que este manda al presentarse (`introduce`), como antes.
- `renew_link` reparte la tarjeta nueva también a los miembros de los círculos de esa lista que
  solo nos conocen por el círculo (no a las solicitudes: retirar el enlace viejo es la idea). Sin
  esto, lo que nos escribían iba a un enlace retirado hasta que les escribiéramos nosotros.
- Sin ficheros ni llamadas en círculos en esta versión (`docs/circulos.md`).


## Estado (2026-09-27, plugins fase 3; `plugins.rs`, `tests/plugins.rs`, `tests/plugin_live.rs`)

Capacidades generales que pedían las notas, la pizarra y el drive:

- **Registros** (`plugin_record*`, tabla `plugin_records`): lo que un plugin guarda más allá de
  sus ajustes, dentro de la cuota concedida (`storage`: 4 MB o 256 MB; un valor 16 MB como mucho).
  Se van con el plugin.
- **Avisos locales** (`set_reminder`, `cancel_reminder`, `reminders`, `due_reminders`; tabla
  `reminders`; permiso `remind`): el núcleo es la verdad y el SO solo el despertador
  (`Event::RemindersChanged` → la app le pasa la lista entera al puente nativo).
- **Refs** (`plugin_ref`, `plugin_ref_target`; tabla `plugin_refs`): un asa opaca al mensaje con
  el que se abrió el plugin; el mismo mensaje da el mismo ref, otro plugin no lo entiende y no
  dice nada del contacto.
- **Canal en directo** (`plugin_live_send`, `Body::PluginEvent`, `Event::PluginEvent`; permiso
  `live` en los dos teléfonos): solo por conexión directa (`transmit_direct`), nunca por el buzón,
  nada se guarda; 48 KiB por mensaje. Lo que llega de un contacto no aceptado o para un plugin no
  instalado o sin el permiso se descarta.
- **Abrir con** (`plugins_opening(mime)`) y `CORE_VERSION` (1.1.0): `install_plugin` rechaza lo
  que pide una versión mayor y `catalogue` no lo ofrece.

## Estado (2026-09-27, la nube del usuario; `vault.rs`, `tests/vault.rs`, `docs/drive.md`)

`Core` lleva el drive de `ft-vault`: `vault_connect(provider, authorizer)` (login por el navegador
que abre la app; los tokens se guardan en `settings` sellados con la clave de almacenamiento),
`vault_reopen` (al arrancar, desde lo que el teléfono guarda), `vault_setup` (devuelve el código
de recuperación, una vez), `vault_unlock(code)`, `vault_disconnect`, `vault_list/mkdir/rename/
move/remove/upload/run_queue/cancel_pending/download/send`, `vault_backup` (instantánea de la base
de datos + clave + carpeta de ficheros), `vault_restore` (a la carpeta de mudanza: la app la cambia
al arrancar), `plugin_may_use_drive`. `trait Cloud` abstrae el proveedor (`GoogleCloud` en la
app, una memoria en los tests) y `trait Authorizer` el navegador. `Event::VaultChanged` y
`Event::VaultProgress`.

## Estado (2026-09-28, recuperar la cuenta; `plan-recuperacion`, `tests/vault.rs`, `tests/conversation.rs`)

- **Frase**: `vault_setup(frase)`, `vault_unlock(frase)`, `vault_change_phrase`,
  `vault_suggest_phrase`. La frase no se guarda. Cinco frases malas seguidas (`WrongPhrase`)
  bloquean la recuperación **24 h en este teléfono** (`vault.tries`); ni la buena abre hasta
  entonces. `VaultStatus` lleva `tries_left` y `retry_at`, y el estado `Outdated` para un drive de
  la versión 1 (también si el teléfono guardaba su clave): `vault_setup` lo rehace.
- **Sesiones tras restaurar** (`§5` del plan): `vault_restore` marca la copia (`sessions_behind`);
  `Core::open` con esa marca renueva el canal de cada contacto (`Channel::renew`) y pone
  `card_stale`, así que `online::start` reparte la tarjeta por las sesiones nuevas. Test de
  aceptación: copia, siguen hablando, se pierde el teléfono, se restaura y los dos se leen en ambos
  sentidos (sin la renovación, el restaurado no lee al otro). Probado en el Lenovo desinstalando la
  app (`docs/drive.md`).

## Estado (2026-09-28, llamadas de voz nativas; `native_calls.rs`, `tests/native_calls.rs`)

En los teléfonos, la **voz** de una llamada ya no va por el WebView sino por Rust (`ft-media`, sobre
`webrtc-engine`): con el iPhone bloqueado, CallKit contesta sin WebView y la media de WKWebView se
silencia en segundo plano. El vídeo y el escritorio siguen con el WebView. La señalización no
cambia (`CallOffer`, `CallAnswer`, `CallEnd`, SDP estándar), así que un teléfono nativo habla con un
WebView (una app vieja): a una oferta con vídeo se le contesta solo el audio (`m=video 0`).

- `start_native_call(contacto, ruta)` devuelve el id al momento; en segundo plano abre la conexión
  (una pista de audio Opus), junta los candidatos (sin goteo) y manda la oferta con `offer_call`.
  `call_answered` pone la respuesta en esa conexión. `answer_native_call(llamada, ruta)` contesta
  con la oferta que se guardó al sonar (`ringing_offer`); contestar dos veces (CallKit y el WebView)
  no hace nada. `answer_ringing_call` es el «contestar» del sistema: la llamada de voz que suena,
  con la ruta guardada (la de vídeo se deja al WebView).
- **Ruta** (`§17`): `set_call_routing`/`call_routing` (ajuste `call_routing`), una copia de la de
  Ajustes para lo que se contesta sin WebView. `direct` quita el TURN, `always` solo TURN y política
  `relay`. STUN, TURN y direcciones salen de `Transport::media_config` (el de `Network`: el mismo que
  los DataChannels).
- Al conectar: `CallUpdate::Connected` y arranca la voz; si la conexión falla, la llamada acaba como
  `failed`, y también si el micrófono o el altavoz no abren. `close_call` (cualquier final, de
  cualquier lado) para la voz y cierra la conexión. `mute_call`/`mute_current_call` →
  `CallUpdate::Muted`.
- **Sesión de audio de iOS**: `set_call_audio_active` (el `didActivate` de CallKit). Con
  `Activation::WhenSessionActive` el dispositivo arranca solo con la llamada conectada **y** la
  sesión activa; se para si CallKit la retira y vuelve a arrancar si la devuelve. En Android arranca
  al conectar.
- `current_call()`: la llamada que suena o va (fase, oferta si suena, `native`, `muted`,
  `connected_at`), para un WebView que llega tarde. `set_call_audio(None)` deja todo en el WebView
  (escritorio; por defecto, `ft_media::platform_audio()`).
- Tests (WebRTC real en loopback, dispositivos falsos que hablan una voz de prueba): la voz llega a
  los dos lados (correlación ~0,97), silenciar deja silencio (RMS ~0), colgar para los dos, el
  dispositivo de iOS espera a CallKit y una oferta de WebView con vídeo se contesta solo con audio.
