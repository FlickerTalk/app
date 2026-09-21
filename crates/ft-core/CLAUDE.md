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
