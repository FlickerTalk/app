# app/crates/ft-protocol

Wire protocol de FlickerTalk: tipos y (de)serialización. Sin I/O, sin red, sin almacenamiento.

## Formatos

- **`SignalEnvelope`** (`§14`) — viaja por push: `version`, `type`, `session_id`, `sender_id`,
  `payload` (cifrado para el receptor), `signature`. Tipos: `WAKE`, `OFFER`, `ANSWER`, `ICE`,
  `CANCEL`.
- **`FlickerPacket`** (`§23`) — viaja por DataChannel: `protocol_version`, `packet_type`,
  `message_id`, `timestamp_local`, `payload`, `signature`. Tipos iniciales: `HELLO`, `MESSAGE`,
  `DELIVERED`, `READ`, `TYPING`, `PING`, `PONG`, `CONTACT_CARD`, `BLOCK`, `FILE_META`,
  `FILE_CHUNK`.
- **`FlickerContactCard`** (`§32`) — `device_id`, `public_identity_key`, `routing_capability`,
  `version`, `signature`.

## Reglas

- Codificación **CBOR**, no JSON. El signaling debe ser **pequeño** por los límites de FCM/APNs
  (`§15`): CBOR + compresión + trickle ICE.
- **Todo va versionado y es compatible hacia atrás** (`§23`): un cambio de formato nunca rompe a
  un peer con una versión anterior.
- `message_id` = **UUIDv7** generado en el cliente, nunca por un servidor (`§24`).
- Canales lógicos (`control`, `messages`, `receipts`, `files`, `plugin-events`) multiplexados al
  principio sobre un único DataChannel (`§22`).
- Ficheros: `FILE_META` + `FILE_CHUNK` (chunks con hash propio, reanudables y BLAKE3 final,
  `§62–63`). Las llamadas usan pistas de media de WebRTC; sus mensajes de control (llamar,
  colgar…) están por definir (`§66`).
- La preferencia de buzón de cada usuario (`§19`) viaja entre dispositivos, nunca al servidor:
  en la `FlickerContactCard` al emparejar y en un paquete de control por P2P cuando cambia
  (campo y tipo de paquete por definir).
