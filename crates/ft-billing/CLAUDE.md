# app/crates/ft-billing

Periodo gratuito, clase de edad y suscripción, todo **local** (`§39–47`).

## Modelo (decidido 2026-09-21, `§40–42`)

- Primer año gratis desde la instalación; después **1 €/año** para adultos.
- Menores de 18: **siempre gratis**.
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

## Reglas

- Edad, entitlement y paywall (fase 4, `§93`) no bloquean el lanzamiento, pero deben estar listos
  antes de que acabe el primer año gratuito de los primeros usuarios.
- Pendiente antes de activar el cobro (`§40`): si un adulto sin suscripción puede seguir
  recibiendo y respondiendo.
- **Nunca** fecha de nacimiento, y `age_class` no se envía al servidor.
- Ningún dato de pago (tarjeta, IBAN, dirección, titular): lo gestiona la Store (`§47`).
- El backend no sabe quién paga (`§45–46`).
