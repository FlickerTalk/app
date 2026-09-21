# app/crates/

Núcleo Rust compartido de la app (`Plan.md §82`). Toda la lógica de negocio del cliente vive
aquí.

## Reglas

- `ft-core` orquesta; los demás crates tienen una responsabilidad cada uno y no dependen de
  `ft-core`.
- Ningún crate depende de `app/src-tauri` ni de `server/`.
- Sin código de plataforma: lo que exige Kotlin/Swift (push nativo, agenda, billing) llega por
  el bridge de `app/src-tauri`.
- Los secretos (clave privada, push token, claves de sesión) no salen de estos crates hacia la
  UI ni hacia los plugins (`§54`).
