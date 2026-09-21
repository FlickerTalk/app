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
