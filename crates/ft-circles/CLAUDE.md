# app/crates/ft-circles

La **tarjeta de círculo** (2026-09-27): quién está en un círculo, quién lo administra, su nombre
y si solo escriben los administradores. Sin I/O y sin criptografía nueva: la firma Ed25519 de la
identidad (`ft-identity`) y las Contact Cards de `ft-contacts`.

## Qué es

- `CircleCard`: cuerpo CBOR firmado (`version`, `id` aleatorio de 16 bytes en hex, `revision`,
  `name`, `admins`, `members`, `admins_only`, `created_at`) más la clave del administrador que
  firmó y su firma. Cada `Member` lleva su propia `ContactCard` firmada: quien entra en un círculo
  puede abrir un canal Olm con cada miembro **sin escanear a nadie**.
- **Cadena de revisiones**: un teléfono sustituye la tarjeta que tiene solo por una `revision`
  mayor firmada por un administrador **de la tarjeta que tiene** (`judge`). Así un miembro no
  puede añadirse a sí mismo, nombrarse administrador ni echar a nadie. La revisión nueva puede
  adelantar como mucho `MAX_REVISION_STEP` (1000) a la que se tiene (revisión del 2026-09-27): un
  administrador no puede apoderarse de un número inalcanzable ni acercarse a `u64::MAX`, y
  `revise` nunca da la vuelta. Todo administrador tiene el mismo poder: puede quitar a los demás. Una tarjeta de un círculo
  desconocido se acepta solo si la firma uno de sus propios administradores (`stands_alone`).
- **Empate**: dos administradores que firman la misma revisión a la vez se resuelven por el id
  de dispositivo más bajo, igual en todos los teléfonos.
- Límites: 32 miembros (`MAX_MEMBERS`), nombre de 40 caracteres (`NAME_LIMIT`), al menos un
  administrador, y todo administrador es miembro. `revise` consolida los cambios y firma la
  siguiente revisión; solo la acepta si quien firma es administrador.

## Reglas

- La tarjeta viaja **siempre cifrada de extremo a extremo** (`Body::CircleCard`), nunca en claro
  por el router. El servidor no sabe que existe un círculo.
- No hay canales públicos ni suscripción por enlace: alguien entra porque un administrador lo
  mete, y se le identifica por su tarjeta de contacto.
