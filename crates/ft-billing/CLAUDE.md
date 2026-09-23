# app/crates/ft-billing

Periodo gratuito, clase de edad y suscripción, todo **local** (`§39–47`).

## Modelo (decidido 2026-09-21, `§40–42`)

- Primer año gratis desde la instalación; después **1 €/año** para adultos.
- Menores de **21**: siempre gratis (decisión de Ioan, 2026-09-22).
- Se anuncia desde el lanzamiento.

## Responsabilidades

- **Trial** (estrategia A, `§41`): `trial_started_at` guardado en local desde la primera ejecución
  (**entra en el MVP**); 365 días de acceso completo sin tarjeta. Se acepta que reinstalar pueda
  reiniciarlo.
- **Edad** (`§43–44`): `AgeClass { Minor, Adult, Unknown }` a partir de la señal del SO (Declared
  Age Range en iOS, Play Age Signals en Android) o, si no la hay, declaración local del usuario.
- **Entitlement** (`§45`): StoreKit 2 (current entitlements) / Google Play Billing (active
  purchases), verificado y cacheado en local.
- Decisión de acceso tras el trial (`§42`): menor → gratis; adulto → entitlement o paywall.

## Estado (2026-09-23)

Hecho y probado: `Access::of(now, Plan)` decide entre `Trial`, `Young`, `Subscribed` y `Limited`,
y `may(Doing)` dice qué se puede hacer. El núcleo lo aplica en `send_text` (responder sí, empezar
no), `send_file` y `place_call`, guarda la clase de edad y lo que diga la Store, y la app tiene su
pantalla de Plan (Ajustes → Plan): cuánto queda del año, el euro y la declaración de edad.

**Falta solo la compra**: Google Play Billing y StoreKit 2 necesitan un producto dado de alta en
la consola de cada tienda, que no existe hasta publicar. El puente ya está (`subscribe`,
`subscription` en `src-tauri/platform`): hoy Kotlin contesta «todavía no» en vez de fingir, y al
arrancar la app pregunta a la Store qué sabe.

## Reglas

- Edad, entitlement y paywall (fase 4, `§93`) no bloquean el lanzamiento, pero deben estar listos
  antes de que acabe el primer año gratuito de los primeros usuarios.
- **Decidido (Ioan, 2026-09-23):** un adulto sin suscripción **sigue recibiendo y respondiendo**;
  lo que no puede es **empezar** una conversación nueva, llamar ni enviar ficheros. Nadie pierde
  un mensaje por no pagar (`§1`).
- **Nunca** fecha de nacimiento, y `age_class` no se envía al servidor.
- Ningún dato de pago (tarjeta, IBAN, dirección, titular): lo gestiona la Store (`§47`).
- El backend no sabe quién paga (`§45–46`).
