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
  `close_session` borra la sesión si está vacía (sin contactos ni solicitudes) y devuelve si se
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
- Sin ficheros ni llamadas en círculos en esta versión (`docs/circulos.md`).

