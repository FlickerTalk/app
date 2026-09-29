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

- **Cliente OAuth de Google** (2026-09-27): cliente de tipo Android en el proyecto de Firebase, con
  «Enable custom URI scheme» activado (Google lo marca como no recomendado en Android; así un solo
  cliente vale para cualquier firma), redirección `com.googleusercontent.apps.<id>:/oauth2redirect`.
  Se pasa en `FT_GOOGLE_CLIENT_ID` al compilar. La pantalla de consentimiento está **en pruebas**:
  solo entran los usuarios de prueba dados de alta; falta la verificación de Google para publicar.
  Pasar Android a `AuthorizationClient` de Play Services y el cliente de iOS van después de
  producción (decisión de Ioan).
- **Swift** (`authorize` en iOS) sin compilar: no hay Xcode aquí. El Kotlin está probado.
- Restaurar una copia en otro teléfono, sin probar en dispositivo.
- **Subidas largas en segundo plano** (servicio en primer plano en Android): pendiente; hoy una
  subida grande necesita la app en pantalla y, si Android la mata, queda pendiente y se reintenta.
- **Pago** (producto `drive`, fase 6 del plan), Dropbox, iCloud: fuera de esta entrega.
- Pruebas automáticas contra Google real (`tests/live_*`): pendientes.
