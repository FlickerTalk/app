# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Estado del repositorio

Proyecto en **esqueleto** y de **código abierto**: AGPL-3.0, salvo el plugin SDK, que es MIT
(`§104`). Cada carpeta tiene su propio `CLAUDE.md` con su responsabilidad, sus reglas y, si los
hay, sus comandos; este fichero solo recoge lo transversal.

- **`Plan.md`** —el documento de diseño completo y **fuente de verdad** de todas las
  decisiones— vive en el repo **privado** `FlickerTalk/flickertalk-ops`, que en local está en
  `../flickertalk-ops/` junto con la infraestructura. Las referencias `§N` de estos `CLAUDE.md`
  apuntan a él. Antes de proponer o implementar algo, lee la sección relevante; las decisiones
  cerradas están en `§102`. Si una decisión cambia, se actualiza `Plan.md`; no se contradice en
  silencio.
- **Nada del repo privado** (plan, infraestructura, secretos) se copia a este repo público.
- Único código existente: la plantilla de `create-tauri-app` en `app/` (compila, con Android
  inicializado). Comandos en `app/CLAUDE.md`. Aún no hay workspace Cargo ni tests.
- `.remember/` es estado del plugin *remember*, no forma parte del proyecto (está en
  `.gitignore`).

## Qué es FlickerTalk

Mensajería privada P2P. **Prioridad del producto: que el mensaje se entregue, con privacidad.**
El historial vive **solo en los dispositivos** (SQLite local) y los mensajes viajan por
**WebRTC DataChannel** siempre que se pueda. El servidor (`api.flickertalk.com`) no es un
servidor de chat: guarda `device_id → push target cifrado` para despertar al receptor vía
FCM/APNs, hace de canal de signaling (los pushes transportan OFFER/ANSWER/ICE cifrados) y, si no
hay conexión P2P, mantiene un **buzón efímero** con el mensaje cifrado E2EE hasta que el
destinatario lo recoge (`§19`).

**Funciones base: texto, envío de ficheros, llamada de voz y videollamada, siempre 1 a 1. Nada
más** (`§85`). Ficheros y llamadas son solo P2P: nunca pasan por el buzón (`§62`, `§66`).

## Invariantes (no romper nunca)

Todo cambio pasa por el filtro de `§100`: *¿necesitamos realmente guardar este dato en nuestro
servidor?* Si no → no se guarda.

- **El servidor nunca puede leer un mensaje ni guarda historial.** Persistencia del servidor:
  `device_id`, `push_provider`, `push_target` (cifrado con clave maestra externa a la BD),
  opcionalmente `updated_at`, y el buzón efímero (solo blobs E2EE, se borran al recogerse o al
  caducar —TTL provisional 7 días—, nunca en backups). No existen endpoints `/messages`,
  `/conversations`, `/users`, `/profiles`, `/history`, `/attachments` (`§8–10`, `§19`).
- **Entrega fiable (`§17`, `§19`):** P2P primero; si no conecta, buzón cifrado. TURN propio
  pendiente de los datos del PoC 2. El mensaje sigue en el `pending_outbox` del emisor hasta
  recibir `delivered`, y nunca se marca como entregado algo que no lo está (`§84`).
- **Buzón opcional, activado por defecto:** el usuario puede desactivarlo en los ajustes, y
  entonces no se guarda nada suyo en el servidor. Un mensaje solo va al buzón si emisor **y**
  destinatario lo tienen activado; si no, espera en el teléfono del emisor (P2P estricto). El
  servidor **no guarda ninguna preferencia**: cada petición lleva el indicador `store` y el
  servidor se fía de él. La preferencia del destinatario llega al emisor por P2P y por la
  Contact Card (`§19`).
- **Push = wake + signaling cifrado**, nunca contenido. Si hace falta un blob de signaling
  temporal: solo en memoria, TTL 30–60 s, jamás en base de datos ni disco (`§12`, `§15`).
- **Identidad criptográfica, sin cuentas.** Clave privada solo en Keystore/Keychain/secure store;
  `device_id = BLAKE3(public_identity_key)`. Las peticiones al router van firmadas
  (`device_id`, `timestamp`, `nonce`, `signature`) y `wake` exige `route_capability` (256 bits
  aleatorios, viaja en la Contact Card) para impedir spam/enumeración (`§6–7`, `§34`).
- **Nada de criptografía casera.** Protocolos y bibliotecas revisadas. El buzón obliga a E2EE
  **a nivel de mensaje**, asíncrono y con forward secrecy (tipo Double Ratchet o MLS), además del
  DTLS de WebRTC (`§28`).
- **Logs y métricas:** sin access logs, sin cuerpos, sin `device_id`/push token/IP en logs ni
  como etiquetas de métricas. Sin Sentry, analytics ni Firebase Analytics (`§71–72`).
- **Datos locales que nunca salen:** agenda del teléfono, `age_class` (enum
  `Minor/Adult/Unknown`, nunca fecha de nacimiento), estado de suscripción (verificación de
  entitlement local vía StoreKit 2 / Play Billing; ningún dato de pago) (`§30`, `§43–47`).
- **Contact discovery v1** = Contact Card firmada por QR / share sheet / deep link. No hay índice
  `teléfono → device_id` en el servidor (`§31–32`).

## Arquitectura planificada

Monorepo (`§81–82`), cuatro carpetas en este repo, más la infraestructura en el privado:

- `app/` — todo el cliente: app Tauri 2 de `create-tauri-app` (`src/` UI, `src-tauri/`), el
  núcleo Rust en `app/crates/ft-*` (`ft-core` orquesta; `ft-identity`, `ft-crypto`,
  `ft-protocol`, `ft-webrtc`, `ft-storage`, `ft-push`, `ft-contacts`, `ft-billing`,
  `ft-plugins`) y los paquetes TS `app/packages/{ui,plugin-sdk}`. **Poca lógica de negocio**
  fuera de los crates.
- `server/ft-router/` — push router y buzón: Axum + Tokio + PostgreSQL, réplicas sin estado.
- Infraestructura y despliegue: en el repo privado (`../flickertalk-ops/infra/`, `§75`).
- `web/` — landing estática `flickertalk.com` con las páginas legales; sin trackers.
- `plugins/` — plugins web desarrollados fuera de la app (cada uno autocontenido, puede tener su
  propio repo); la app los instala como paquetes firmados.

Capas del cliente (`§5`): UI (Lit + Web Components + TypeScript; **no** React/React
Native/Angular) → Plugin Runtime → Rust Core → Platform bridge. Kotlin/Swift solo donde el SO
lo obligue (FCM/APNs, contactos, billing).

Flujo de envío (`§18–19`, `§26–27`):

1. Se escribe en SQLite local (`state = pending`) y en `pending_outbox`.
2. Si hay DataChannel abierto → se envía → el receptor inserta y responde ACK → `delivered`.
3. Si no → `WAKE` vía router → FCM/APNs → signaling (`OFFER`/`ANSWER`/`ICE`) por push →
   DataChannel → envío.
4. Si P2P no conecta → blob E2EE al buzón del destinatario (`sent`) → push → el destinatario lo
   recoge (en iOS, notificación visible + Notification Service Extension), lo guarda y hace ACK
   → el servidor lo borra → `delivered` (`§19–20`).
5. `message_id` es UUIDv7 generado en el cliente; el receptor deduplica por `message_id UNIQUE`
   y **siempre** responde ACK, así los reintentos son idempotentes.

Protocolo: `SignalEnvelope` (push) y `FlickerPacket` (DataChannel) en CBOR, **siempre
versionados** y compatibles hacia atrás (`§14`, `§23`).

Plugins (`§48–58`): solo web (HTML/CSS/JS/Web Components), nunca nativos. Firma BLAKE3 + Ed25519
verificada **antes** de ejecutar nada. Un plugin nunca recibe claves ni push token, red
desactivada por defecto (CSP), y nunca hace `invoke` directo de Tauri: siempre
`plugin → Plugin API → permission check → Rust Core`. En iOS los plugins descargados tienen un
sandbox más limitado (App Store 4.7).

## Orden de trabajo

La prioridad absoluta es el **PoC 0** (`§87`, `§103`): Android A → FCM → B despierta → signaling →
Google STUN → DataChannel → `"hello"`; después Android ↔ iPhone. Luego PoC 1 (iPhone bloqueado
en background, `§88`) y PoC 2 (matriz de redes / tasa de éxito solo-STUN, `§89`). **Nada de UI
definitiva antes de esto.** Después, fases 1–9 (`§90–98`). El alcance del MVP está cerrado en
`§85`; lo que queda fuera, en `§86` (grupos, llamadas de grupo, multi-dispositivo, backup
cloud, marketplace…). Al adelantar ficheros y llamadas al MVP, las fases `§90–98` quedan por
reordenar.

Modelo comercial (`§40–42`): primer año gratis desde la instalación (reloj local, sin tarjeta),
después 1 €/año para adultos; menores de 18, siempre gratis. Se anuncia desde el lanzamiento. El
billing no bloquea el MVP, pero debe estar listo antes de que acabe el primer año gratuito.

## Convenciones del proyecto

- Código, identificadores, comentarios, tests y logs en **inglés**. Los `.md` (incluido
  `Plan.md` y este fichero) en español.
- **Interfaz con iconos primero** (`§84`): emoji e iconos estándar en lugar de texto; texto solo
  cuando sea imprescindible, y entonces en **inglés**. Los iconos llevan una etiqueta accesible
  (`aria-label`) en inglés.
- **Sin traducciones en la fase 1** (decisión del proyecto, 2026-09-21): no se crea la
  traducción `es` que pide la regla global; se decidirá más adelante. Los textos pasan igualmente
  por un catálogo i18n con el inglés como fuente, para poder traducir sin tocar los componentes.
  Los textos de ejemplo en español de `Plan.md` se resuelven con un icono o, si hace falta, en
  inglés.
- TDD con el hook global `tdd-guard.sh`: un `.rs` nuevo debe crearse ya con su
  `#[cfg(test)] mod tests` dentro (también `main.rs`/`lib.rs` al hacer el scaffolding); en
  TypeScript, crea antes `<module>.test.ts`.
