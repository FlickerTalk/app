# app/crates/ft-contacts

Contactos, emparejamiento y moderación **local** (`§29–36`).

## Responsabilidades

- Emparejar mediante **Contact Card** firmada intercambiada por QR, share sheet o deep link (NFC
  más adelante) (`§32`). Es el único mecanismo de descubrimiento en v1.
- Asociar localmente una identidad FlickerTalk a un contacto de la agenda del teléfono (`§30`).
- Fingerprint de seguridad y verificación por QR en persona (`§29`).
- Bloqueo local por `device_id` (`§35`).
- Reporte básico: `reported_device_id` + `reason`; adjuntar un mensaje como evidencia solo por
  acción explícita del usuario (`§36`).

## Reglas

- La agenda se lee **solo en local** y nunca se sube; el permiso se pide cuando hace falta
  (`§30`).
- No existe índice `teléfono → device_id` en ningún servidor (`§31`). El descubrimiento privado
  (PSI/OPRF) es un proyecto aparte y posterior (`§33`, `§96`): no se implementa aquí.
- Los reportes van a infraestructura **separada** de la de mensajería (`§36`).

## Estado (2026-09-22, `§106` M1)

`ContactCard`: nombre sugerido, clave Ed25519, clave Curve25519, fallback key, route capability y
preferencia de buzón, en CBOR y firmada con la identidad. Viaja como
`https://flickertalk.com/add#<base64url>` (el fragmento no llega a ningún servidor), menos de 600
caracteres para el QR. `RouteCapability`: 256 bits aleatorios; el router solo verá su BLAKE3.
