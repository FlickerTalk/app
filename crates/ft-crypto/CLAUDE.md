# app/crates/ft-crypto

Protocolos criptográficos del cliente (`§28`).

## Responsabilidades

- **E2EE a nivel de mensaje**, asíncrono y con forward secrecy (tipo Double Ratchet o MLS).
  Obligatorio desde el buzón (`§19`): los mensajes pueden esperar cifrados en el servidor. Queda
  por diseñar el intercambio inicial de claves asíncrono (prekeys).
- Handshake firmado entre peers con la identity key permanente, por encima del DTLS de WebRTC,
  para que el signaling no pueda sustituir al peer en silencio.
- Cifrar el `payload` de signaling para el receptor (`§14`).
- Fingerprints de seguridad de contacto (`§29`).
- Más adelante: backup cifrado con clave derivada de una contraseña del usuario (`§61`).

## Reglas

- **Nada de criptografía casera.** Solo protocolos conocidos y bibliotecas revisadas; este crate
  las envuelve, no implementa primitivas. Nunca «he inventado este AES + hash».
- Candidatas a evaluar (`§28`): `vodozemac` (Apache-2.0), `OpenMLS` (MIT), `libsignal`
  (AGPL-3.0, condiciona la licencia del cliente).
- Todo lo de aquí entra en la revisión de protocolo y cripto previa a producción (`§94`).

## Estado (2026-09-22, `§106` M1)

Olm de `vodozemac` 0.11, sesiones versión 1 (la 2 es experimental). `contact_keys` da la clave
Curve25519 y la *fallback key* para la Contact Card (nunca se marca como publicada, para que la
tarjeta no cambie). `Channel` guarda las sesiones con un contacto, descifra con la que encaje y
cifra con la última que funcionó, así que dos contactos que se escanean a la vez convergen.
`accept_first_contact` abre el primer mensaje de un desconocido con la clave que trae su pre-key
message; quien llama debe comprobarla contra la tarjeta que viene dentro. Un mensaje repetido
byte a byte se rechaza (protección contra *replay*).
