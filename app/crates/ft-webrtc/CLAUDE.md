# app/crates/ft-webrtc

Conexiones P2P: `PeerConnection`, ICE y `DataChannel` en Rust, **sin depender de la WebView**
(`§21`). Es el camino preferente para entregar mensajes; si falla, el core usa el buzón cifrado
(`§19`).

## Responsabilidades

- DataChannel `ordered = true`, `reliable = true` (`§22`).
- ICE con el STUN de Google `stun:stun.l.google.com:19302` (`§16`) y trickle ICE (`§15`).
- Producir y consumir SDP y candidatos ICE. El **transporte** del signaling no es de este crate:
  va por push a través de `ft-push` (`§13`).
- Reconexión.
- Pistas de audio y vídeo para las llamadas 1 a 1 (`§66`). Falta decidir en el PoC si la
  captura va por la WebView o por Rust con captura nativa.
- Informar al core de si la conexión se establece o no, en un tiempo acotado, para que pueda
  recurrir al buzón.

## Reglas

- **TURN pendiente del PoC 2** (`§17`, `§89`): si se añade, será un TURN propio sin
  almacenamiento. Hasta entonces, solo STUN. Las llamadas aumentan la necesidad de TURN (`§66`).
- Implementación candidata: crate `webrtc` (documentado en 0.21.0). Hay que validarla en
  Android, iOS, wake en background, DataChannel, STUN y reconexión **antes de construir encima**:
  es el PoC 0/1 (`§87–88`).
- El P2P directo expone las IPs públicas entre peers (`§67`): es parte del modelo y se documenta,
  no se oculta.
