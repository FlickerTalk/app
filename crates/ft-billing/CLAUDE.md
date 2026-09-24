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

## Decisión (2026-09-24): de dónde sale `until` en cada tienda

Las dos tiendas responden `until` (ms), pero **no saben lo mismo**:

- **Apple (StoreKit 2)** dice **hasta cuándo**: `Transaction.expirationDate`. Se guarda esa fecha
  tal cual, y `revocationDate` (devolución) deja de contar en el acto.
- **Google (Play Billing)** solo dice **cuándo se pagó**: el `Purchase` del cliente lleva
  `purchaseTime`, no la caducidad. La caducidad real solo la da la Play Developer API **desde un
  servidor**, y eso sería que nuestro backend supiera quién paga (`§45–46`). Así que en Android
  `until = purchaseTime + 365 días`, y cada renovación mueve la fecha porque llega con un
  `purchaseTime` nuevo.

Consecuencia asumida: en Android la fecha puede desviarse algún día respecto de la de Google
(años bisiestos, reintentos de cobro). Da de más, nunca de menos, y nadie pierde acceso por ello.
Una compra **pendiente** (efectivo, transferencia, Ask to Buy) **no cuenta** hasta que se paga
(`§84`).

## Estado (2026-09-23)

Hecho y probado: `Access::of(now, Plan)` decide entre `Trial`, `Young`, `Subscribed` y `Limited`,
y `may(Doing)` dice qué se puede hacer. El núcleo lo aplica en `send_text` (responder sí, empezar
no), `send_file` y `place_call`, guarda la clase de edad y lo que diga la Store, y la app tiene su
pantalla de Plan (Ajustes → Plan): cuánto queda del año, el euro y la declaración de edad.

**Estado de la compra (2026-09-24):** el puente ya **compra de verdad** en los dos sistemas.
Android usa `com.android.billingclient:billing:9.1.0` (la variante Java: la `-ktx` viene compilada
con Kotlin 2.3 y el módulo va con 1.9) y iOS usa StoreKit 2. Lo que falta **no es código**: el
producto `yearly` no existe todavía en ninguna de las dos consolas. Play no deja crearlo hasta
subir un paquete que lleve la librería de facturación —por eso iba antes el código que el
producto— y App Store Connect necesita la cuenta de pago de Apple.

## Reglas

- Edad, entitlement y paywall (fase 4, `§93`) no bloquean el lanzamiento, pero deben estar listos
  antes de que acabe el primer año gratuito de los primeros usuarios.
- **Decidido (Ioan, 2026-09-24): `Unknown` cuenta como adulto.** Si no consta la edad, se pide el
  euro igual que a un adulto: lo contrario —desconocido gratis— haría el euro opcional para todo
  el mundo, porque el estado por defecto **es** `Unknown` y nadie está obligado a declarar nada.
  El menor no queda atrapado: el paywall enseña a la vez el euro y el «tengo menos de 21», y en
  una cuenta infantil de verdad la tienda pide permiso al adulto (Ask to Buy → `pending_approval`).
  Fijado en `Access::of` y en su test.
- **Decidido (Ioan, 2026-09-23):** un adulto sin suscripción **sigue recibiendo y respondiendo**;
  lo que no puede es **empezar** una conversación nueva, llamar ni enviar ficheros. Nadie pierde
  un mensaje por no pagar (`§1`).
- **Nunca** fecha de nacimiento, y `age_class` no se envía al servidor.
- Ningún dato de pago (tarjeta, IBAN, dirección, titular): lo gestiona la Store (`§47`).
- El backend no sabe quién paga (`§45–46`).
