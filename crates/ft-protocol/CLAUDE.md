# app/crates/ft-protocol

Wire protocol de FlickerTalk: tipos y (de)serialización. Sin I/O, sin red, sin almacenamiento.

## Formatos (implementados, 2026-09-22)

- **`Packet`** (`§23`): `version`, `id` (UUIDv7), `sent_at` (reloj local, ms) y `body`: `message`,
  `delivered`, `read`, `typing`, `ping`, `pong`, `contact_card`, `mailbox_preference`, `block`. Un
  tipo desconocido se decodifica como `Unknown` y se ignora. Siempre viaja cifrado con Olm: no
  lleva firma propia, porque Olm ya autentica al remitente.
- **`Sealed`**: el `Packet` cifrado (`from`, tipo de mensaje Olm y `ciphertext`), igual por
  DataChannel que por el buzón.
- **`Signal`** (`§14`): señalización de WebRTC entre dos dispositivos (`wake`, `offer`, `answer`,
  `cancel`, `session`, `from`, `to` y el `Sealed` con la descripción). El router solo mueve bytes.
- La **Contact Card** (`§32`) tiene su propio formato firmado en `ft-contacts`.
- **`Envelope`** (2026-09-27, A1): un `Sealed` o un `Signal` sellados otra vez para el destinatario
  (caja sellada NaCl, la hace `ft-identity`), que es lo que ve el router: ni `from` ni clave del
  emisor. Se distingue de un `Sealed` o `Signal` a secas por su campo `envelope`.
- **Relleno** (M9): `Packet::encode` rellena a múltiplos de `PAD_BUCKET` (160 bytes) con un campo
  `pad` que el receptor ignora, para que el tamaño no delate el mensaje.
- **Círculos** (2026-09-27): `circle_card` (la tarjeta firmada de `ft-circles`), `circle_message`
  (`circle`, `text`; el mismo paquete, con el mismo id, va a cada miembro por su canal Olm) y
  `circle_leave`. Un peer anterior los ignora como `Unknown`.

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
