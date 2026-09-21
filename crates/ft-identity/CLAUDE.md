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
