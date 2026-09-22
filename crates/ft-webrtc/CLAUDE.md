# app/crates/ft-webrtc

Conexiones P2P: `PeerConnection`, ICE y `DataChannel` en Rust, **sin depender de la WebView**
(`§21`). Es el camino preferente para entregar mensajes; si falla, el core usa el buzón cifrado
(`§19`).

## Responsabilidades

- DataChannel `ordered = true`, `reliable = true` (`§22`).
- ICE con nuestros STUN y TURN (coturn en los 3 nodos, `§16`), **sin goteo**: la descripción sale
  cuando termina la recogida o, como mucho, a los 3 s con lo reunido; así conectar son solo 2
  señales (oferta y respuesta), lo que mejor encaja con el push (`§15`). El STUN de
  Google es solo de respaldo: WebRTC consulta todos los STUN a la vez, así que se añade a la
  configuración **únicamente si los nuestros no responden** (y durante el PoC).
- Producir y consumir SDP y candidatos ICE. El **transporte** del signaling no es de este crate:
  va por push a través de `ft-push` (`§13`).
- Reconexión.
- Pistas de audio y vídeo para las llamadas 1 a 1 (`§66`). Falta decidir en el PoC si la
  captura va por la WebView o por Rust con captura nativa.
- Informar al core de si la conexión se establece o no, en un tiempo acotado, para que pueda
  recurrir al buzón.

## Reglas

- **TURN solo de respaldo** (`§17`): siempre se intenta primero la conexión directa. Las
  credenciales TURN son temporales y con usuario aleatorio (las pide `ft-push` al router). El TURN
  ve metadatos de esas conexiones, nunca el contenido.
- Implementación candidata: crate `webrtc` (documentado en 0.21.0). Hay que validarla en
  Android, iOS, wake en background, DataChannel, STUN y reconexión **antes de construir encima**:
  es el PoC 0/1 (`§87–88`).
- El P2P directo expone las IPs públicas entre peers (`§67`): es parte del modelo y se documenta,
  no se oculta.

## Estado: PoC 0, fase A superada (2026-09-22)

API: `Session::start(config, Role::Caller | Role::Callee, signals)` devuelve la sesión y su
`Inbox`; `invite()` crea la oferta, `handle_signal()` procesa lo que llega del otro, `wait_open()`,
`send()`, `close()`. Las señales (`Signal::Sdp`) salen por el canal que se pasa en `start`.

- `cargo test -p ft-webrtc`: unitarios y conexión por loopback (dos pares, "hello" en ~0,6 s).
- `cargo test -p ft-webrtc -- --ignored`: la misma conexión por la red real con el STUN de Google.

Aprendizajes de `webrtc` 0.21 (es una reescritura sans-IO, distinta de las 0.1x):

- Eventos con el trait `PeerConnectionEventHandler` (`async_trait`) y el canal con `poll()`. Los
  manejadores no pueden bloquear: el trabajo largo se lanza con `runtime.spawn`.
- **Nunca escuchar en `0.0.0.0` con STUN**: la recogida de candidatos no termina jamás. Se escucha
  en la IP de cada interfaz (`interface_addresses()`, solo IPv4 por ahora).
- Una interfaz que no llega al STUN (un túnel VPN) impide que la recogida se complete: por eso el
  plazo de 3 s y se envía lo reunido. Solo es error no tener ningún candidato.
