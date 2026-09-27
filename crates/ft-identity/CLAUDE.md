# app/crates/ft-identity

Identidad criptográfica del dispositivo (`§6–7`). No hay usuario, email ni contraseña: la
identidad **es** la clave.

## Responsabilidades

- Primera ejecución: generar con `SecureRandom` la clave privada de identidad (Ed25519) y una
  clave apropiada para intercambio de claves/cifrado.
- `device_id = BLAKE3(public_identity_key)`, con formato tipo `ft_74MxNcJ8E2…`. Nunca
  secuencial.
- Guardar la clave en Android Keystore / Apple Keychain / secure store de escritorio.
- Firmar las operaciones sensibles, p. ej. `PUT /v1/device/push` (`device_id`, `timestamp`,
  `nonce`, `push_target`, `signature`).
- Export/import cifrado de la identidad (entra en el MVP, `§85`).

## Reglas

- La clave privada **nunca** sale del dispositivo ni se expone a la UI o a los plugins (`§54`).
- v1: teléfono nuevo = identidad nueva. Multi-dispositivo y migración por QR entre dispositivos
  son posteriores (`§59–60`).

## Estado (2026-09-22, `§106` M1)

La identidad es una cuenta Olm de `vodozemac`: Ed25519 para firmar y Curve25519 para el
intercambio de claves de `ft-crypto`. `DeviceId` = `ft_` + base58(BLAKE3(Ed25519)). `seal`/`unseal`
cifran la cuenta con una clave de 32 bytes que aporta la plataforma; en el teléfono esa clave la
sella Android Keystore / iOS Keychain (`app/src-tauri`, `§94`). Pendiente: el export/import cifrado.

## Estado (2026-09-27, endurecimiento A1)

`EnvelopeKey`: una clave X25519 propia («sobre»), aparte de la cuenta Olm, que viaja en la Contact
Card y con la que los demás sellan lo que mandan por el router (`crypto_box`, caja sellada de
NaCl: nada casero). En reposo va cifrada con XChaCha20-Poly1305 bajo la misma clave de 32 bytes
(`seal_at_rest`/`unseal_at_rest`).
