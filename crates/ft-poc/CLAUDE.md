# app/crates/ft-poc

**Temporal, solo para el PoC 0** (`Plan.md §87`). Conecta una sesión de `ft-webrtc` con el otro
par a través del relay de señalización de `server/ft-router` (WebSocket `/poc/rooms/{room}`). En el
diseño final la señalización va cifrada por push (`§13`, `ft-push`) y este crate desaparece.

- `connect(relay, room, role, config)` → `(Session, Inbox)`. El que llama envía la oferta en cuanto
  el otro está en la sala, llegue quien llegue primero.
- Binario `poc-peer`: el par del Mac. Quien llama dice «hello»; quien responde contesta
  «<mensaje> back». `cargo run -p ft-poc --bin poc-peer -- --room demo --role callee`.
- Tests: `cargo test -p ft-poc` (levanta un relay mínimo dentro del propio test, porque los crates
  de la app no pueden depender de `server/`).
