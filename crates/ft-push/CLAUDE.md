# app/crates/ft-push

Cliente del router `api.flickertalk.com` (`§10`, `§82`): push, wake y buzón.

## Responsabilidades

- Registrar y mantener al día el push target: `POST /v1/device/register`,
  `PUT /v1/device/push` (los tokens FCM/APNs cambian, `§8`) y `DELETE /v1/device`.
- Enviar `WAKE`/signaling a otro dispositivo: `POST /v1/wake/{device_id}` con su
  `route_capability` (`§34`).
- **Buzón** (`§19`): depositar blobs ya cifrados en el buzón del destinatario (con su
  `route_capability`), recoger los propios y confirmar (ACK) para que el router los borre.
- Indicador `store` en **todas** las peticiones al router (`§19`): `true` solo si el usuario y el
  destinatario tienen el buzón activado. El router no guarda preferencias y se fía de él.
- Con el buzón desactivado: no recoger nunca del buzón, no depositar mensajes salientes y, al
  desactivarlo, pedir (firmado) el borrado de los blobs pendientes.
- Recibir el payload de un push entrante y entregarlo al core como `SignalEnvelope`.

## Reglas

- Todas las peticiones van **firmadas** con la identidad (`device_id`, `timestamp`, `nonce`,
  `signature`) (`§7`).
- Un push **nunca** lleva texto, foto, documento ni historial: solo wake, identificador de sesión
  y signaling cifrado (`§12`). El contenido va por P2P o, cifrado, por el buzón.
- Este crate no cifra: recibe blobs ya cifrados por `ft-crypto` y los transporta.
- El token nativo lo obtiene el bridge de plataforma (Kotlin/Swift). Este crate no habla con
  FCM/APNs: eso lo hace el router.
