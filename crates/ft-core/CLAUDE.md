# app/crates/ft-core

Orquestador del cliente (`§82`) y fachada que consume la app Tauri. Coordina identidad,
almacenamiento, protocolo, WebRTC, push, contactos, billing y plugins.

Funciones base (`§85`): texto, ficheros, llamada de voz y videollamada, siempre 1 a 1. Nada más.

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

