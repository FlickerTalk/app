# La nube del usuario: copia de seguridad y «Mi drive»

Decisión del 2026-09-27 (plan del drive, fase A, con los cambios de la valoración: primero la
copia de seguridad, «abrir con» general en vez de una acción específica, sin iCloud). El código de
recuperación generado por la app pasó a ser una **frase que elige el usuario** (decisión del
2026-09-28, `plan-recuperacion`). Implementado en `crates/ft-vault`, `crates/ft-core/src/vault.rs`,
`src-tauri` (`core_vault_*`, `ft.drive.*` en el marco), `src/views/BackupPage.vue` y el plugin
`plugin-drive`.

## Qué es

El usuario guarda en **su propio Google Drive** una copia sellada de su teléfono (historial,
clave y ficheros) y, con el plugin «Mi drive», sus ficheros. Todo se cifra en el teléfono antes de
salir. Google ve ficheros sellados con nombres aleatorios, su tamaño y sus fechas; nosotros no
vemos nada, porque nuestro servidor no participa (`§100`).

## Cómo funciona

1. **Conectar** (Ajustes → Copia de seguridad, o desde el plugin): el núcleo construye la
   dirección de login de Google (OAuth con PKCE, ámbito `drive.file`), el puente nativo la abre en
   el navegador del sistema (Custom Tabs / `ASWebAuthenticationSession`) y trae de vuelta la
   redirección al esquema de la app; el núcleo cambia el código por tokens y los guarda sellados
   con la clave de almacenamiento. La WebView nunca los ve.
2. **Crear el drive** (nube vacía, solo en Ajustes): el usuario escribe dos veces su **frase de
   recuperación** (12 a 100 caracteres; se cuenta recortada y en NFC; la app puede sugerir una de
   30 símbolos, copiarla o compartirla, y avisa de no guardarla en la misma cuenta de Google). La
   clave del drive se genera en el teléfono y se sube sellada con lo que da la frase estirada con
   **Argon2id** (64 MiB, 3 pasadas, sal propia): unos 0,7–1 s en el Lenovo. La frase no se guarda
   en ningún sitio, tampoco en el teléfono. Quien no la configura **no puede recuperar la cuenta**.
   **Abrir el drive de otro teléfono** (nube con drive): con la frase, solo en Ajustes; **cinco
   frases malas seguidas bloquean la recuperación 24 h en ese teléfono** (ni la buena abre hasta
   entonces). El bloqueo solo frena a quien prueba desde la app: lo que protege `key.ftv` en el
   Drive es la frase larga y Argon2id. La frase se puede **cambiar** desde el teléfono que tiene el
   drive abierto; la vieja deja de abrirlo. Un drive de la versión 1 (con código) se rehace.
3. **Copia de seguridad**: instantánea consistente de la base de datos + clave de almacenamiento +
   ficheros, sellados y subidos; la siguiente copia solo sube lo que cambió y borra lo viejo.
   **Restaurar**: en un teléfono nuevo, tras conectar y abrir con la frase, la copia baja a la
   carpeta de mudanza y la app arranca de nuevo con ella, como tras una mudanza (`§60`). Sus
   sesiones Olm son las del día de la copia: el primer arranque abre con cada contacto una sesión
   nueva desde su tarjeta y se la presenta (mensaje *pre-key*), así que los dos lados vuelven a
   leerse y ninguna clave de la copia se reutiliza. Lo dicho después de la copia se pierde con el
   teléfono.
4. **Mi drive** (plugin, permiso `drive`): carpetas, subir del selector, «abrir con» desde
   cualquier fichero de una burbuja (el plugin recibe nombre y tipo, nunca los bytes, y lo guarda
   por la referencia del mensaje), abrir en el visor, guardar en Descargas, enviar a la
   conversación (`send: propose`). Lo que espera red se ve con su motivo.
5. **Olvidar la nube** en este teléfono borra tokens y clave locales; la nube no se toca.

## Qué ve quién

| Quién | Qué ve |
| --- | --- |
| Google | una carpeta `FlickerTalk` con `vault.json` (versión e id aleatorio), `key.ftv`, `index.ftv` y `blob-<id>`: tamaños, fechas y cuántos hay. Nada de nombres ni contenido. Que el usuario usa FlickerTalk. |
| Nuestro servidor | nada: no interviene. |
| El plugin | nombres, tamaños y estados; nunca bytes, tokens ni la frase (crear y abrir el drive no son suyos). |
| La WebView de la app | lo mismo que el plugin, más la frase mientras el usuario la escribe en Ajustes. |

## Recuperar la cuenta, probado en el Lenovo (2026-09-28)

Con la cuenta de Google de Ioan: Mark crea el drive con una frase sugerida y hace copia; Mark y
Lucy (Samsung) se escriben después; se **desinstala** FlickerTalk del Lenovo y se instala de
nuevo; identidad nueva → Ajustes → Copia de seguridad → conectar Google → una frase mala
(«Intentos restantes: 4») → la buena (6,5 s) → restaurar. La app arranca como Mark, con sus
contactos, círculos e historial hasta la copia, y Mark y Lucy se escriben en los dos sentidos con
acuses de entrega y lectura.

## Probado en un Google Drive de verdad (2026-09-27)

En el Lenovo, con la cuenta de Ioan: conectar (Custom Tab → consentimiento `drive.file` → vuelta a
la app), crear el drive, copia de seguridad, y en «Mi drive» guardar un PDF recibido por el chat
(«abrir con»), subir otro desde el selector de Android, bajarlo a Descargas y enviarlo al chat. Lo
bajado y lo recibido en el Samsung es idéntico al original (mismo SHA-256). Los ficheros de la
carpeta de la app, bajados desde el Drive: todos empiezan por `FTV1`, con ~8 bits/byte de
entropía, y ninguno lleva ni el nombre ni un byte del documento.

## Lo que falta y lo que está sin probar en dispositivo

- **Clientes OAuth de Google** (actualizado el 2026-09-30, versión 1.2.2): uno **por plataforma**,
  en el proyecto de Google Cloud `flickertalk-64858`. Los id de cliente de una app instalada son
  identificadores públicos, no secretos, pero no se escriben en el código: se inyectan al compilar.
  - **Android**: cliente de tipo Android con «Enable custom URI scheme» activado (Google lo marca
    como no recomendado en Android; así un solo cliente vale para cualquier firma). Variable
    `FT_GOOGLE_CLIENT_ID`; la lee el núcleo (`option_env!`) y `build.gradle.kts` pone su esquema en
    el manifiesto del puente (`${googleRedirectScheme}`).
  - **iOS** (creado el 2026-09-30): cliente de tipo iOS, bundle `com.flickertalk.app`. Variable
    `FT_GOOGLE_IOS_CLIENT_ID`. En iOS el núcleo usa este y **nunca** cae en el de Android
    (`google_client_for` en `crates/ft-core/src/vault.rs`).
  - **Redirección**, igual en las dos: el id del cliente al revés,
    `com.googleusercontent.apps.<prefijo del id>:/oauth2redirect` (la forma que documenta Google
    para apps instaladas: «OAuth 2.0 for Mobile & Desktop Apps», *Custom URI scheme*). La
    construye `google_login`, con PKCE S256 y solo el ámbito `drive.file`; tests para cada
    plataforma.
  - **iOS no necesita `CFBundleURLTypes`**: `ASWebAuthenticationSession` recibe la vuelta porque el
    puente le pasa el esquema en `callbackURLScheme`. Lo dice la cabecera del SDK de Apple
    (`AuthenticationServices/ASWebAuthenticationSession.h`, iOS 27): «For the app to receive the
    callback URL, it needs to either register the custom URL scheme in its Info.plist, or set the
    scheme to callbackURLScheme argument in the initializer». Por eso `Info.ios.plist` no cambia.
  - **Dónde están**: `infra/.env` (`FT_GOOGLE_CLIENT_ID`, `FT_GOOGLE_IOS_CLIENT_ID`; el JSON del
    cliente de Android en `infra/secrets/google-oauth-android-client.json`) y, para CI, los dos
    como secretos del entorno `release` de GitHub. El job de Android recibe `FT_GOOGLE_CLIENT_ID`;
    no hay job de iOS (se compila en un Mac con la variable exportada).
  - **`tauri ios build` no pasa las variables**: la CLI de Tauri (2.11, `mobile::env_vars`) da al
    build de Xcode, y con él a cargo, un entorno limpio con solo `TAURI*`, `WRY*`, `CARGO_*`,
    `RUST_*`, `TMPDIR` y `PATH`. Por eso el núcleo y la comprobación leen también
    `TAURI_FT_GOOGLE_IOS_CLIENT_ID` (y `TAURI_FT_GOOGLE_CLIENT_ID`, `TAURI_FT_ALLOW_NO_GOOGLE_CLIENT`);
    el nombre sin prefijo gana. Para iOS: `export TAURI_FT_GOOGLE_IOS_CLIENT_ID=$FT_GOOGLE_IOS_CLIENT_ID`
    antes de `tauri ios build` (`scripts/ios-build.sh` lo hace solo). En Android, Gradle hereda el
    entorno entero y basta `FT_GOOGLE_CLIENT_ID`.
  - **Comprobación al compilar** (`src-tauri/build.rs`, lógica y tests en
    `src-tauri/google_client_check.rs`): una build **release** para Android o iOS **no compila**
    si falta el id de su plataforma o no tiene la forma `<id>.apps.googleusercontent.com`; una
    **debug** solo avisa. Una release que nunca se publica (el build sin firmar de un PR, sin
    secretos) lo dice con `FT_ALLOW_NO_GOOGLE_CLIENT=1`. Las builds de tienda de la 1.2.1 salieron
    sin el id y «Conectar Google Drive» fallaba en las dos plataformas.
  - **Pantalla de consentimiento**: **en producción** desde el 2026-09-30, solo con el ámbito no
    sensible `https://www.googleapis.com/auth/drive.file`, así que entra cualquier cuenta sin
    verificación de Google. La app pide ese ámbito y ningún otro (test). Pasar Android a
    `AuthorizationClient` de Play Services queda para después.
- El login en iOS se probó en el simulador hasta el selector de cuentas de Google (2026-09-30);
  el ciclo completo con una cuenta, en el iPhone, sin probar.
- Restaurar una copia en otro teléfono, sin probar en dispositivo.
- **Subidas largas en segundo plano** (servicio en primer plano en Android): pendiente; hoy una
  subida grande necesita la app en pantalla y, si Android la mata, queda pendiente y se reintenta.
- **Pago** (producto `drive`, fase 6 del plan), Dropbox, iCloud: fuera de esta entrega.
- Pruebas automáticas contra Google real (`tests/live_*`): pendientes.
