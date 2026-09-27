# app/crates/ft-storage

Base de datos **local** del dispositivo: SQLite con `sqlx` (`§25`). Todo el historial vive aquí y
en ningún otro sitio.

## Tablas (2026-09-22)

`identity` (cuenta Olm sellada, route capability y clave de sobre sellada), `contacts` (tarjeta,
sesiones Olm selladas, buzón, bloqueo, `introduced`, reglas, sesión y `accepted`), `messages`
(`sent_at` del emisor y `received_at` de este teléfono, que manda en orden y retención),
`pending_outbox`, `settings`, `files` (con `waiting`), `calls`, `plugins`, `plugin_memory`,
`sessions` y `spare_capabilities`. Una conversación 1 a 1 es su contacto, así que no hay tabla
`conversations`. Migraciones en `migrations/`, incrustadas en el binario; la `0010_hardening.sql`
trae las columnas del paquete de endurecimiento (2026-09-27).

## Reglas

- `messages.message_id` es **UNIQUE**: la inserción es idempotente para que los reintentos del
  emisor, por P2P o por el buzón, sean seguros (`§27`).
- `pending_outbox` (`message_id`, `target_device`, `created_at`, `retry_state`, `next_attempt`):
  un mensaje sigue aquí hasta recibir `delivered`, aunque ya esté en el buzón del servidor (`§19`,
  `§26`).
- Estados de mensaje `pending` → `sent` (en el buzón o transmitido por P2P) → `delivered` →
  `read`, todos locales (`§19`, `§38`).
- Una conversación se identifica localmente por sus participantes (`§25`).
- `settings` guarda si el usuario usa el buzón (`§19`). Los ficheros recibidos se guardan en
  local y nunca se suben a ningún servidor (`§62`).
- El estado local de billing/edad (`trial_started_at` desde la primera ejecución, `age_class`,
  caché de entitlement) también se persiste aquí; nunca una fecha de nacimiento (`§41–45`).
