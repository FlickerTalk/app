# La nube del usuario: copia de seguridad y «Mi drive»

Decisión del 2026-09-27 (plan del drive, fase A, con los cambios de la valoración: primero la
copia de seguridad, «abrir con» general en vez de una acción específica, sin iCloud, código de
recuperación generado por la app). Implementado en `crates/ft-vault`, `crates/ft-core/src/vault.rs`,
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
2. **Crear el drive** (nube vacía): se genera la clave del drive en el teléfono, se sube sellada
   con el **código de recuperación** (30 símbolos, generado por la app) y el código se enseña una
   sola vez. **Abrir el drive de otro teléfono** (nube con drive): con ese código.
3. **Copia de seguridad**: instantánea consistente de la base de datos + clave de almacenamiento +
   ficheros, sellados y subidos; la siguiente copia solo sube lo que cambió y borra lo viejo.
   **Restaurar**: en un teléfono nuevo, tras conectar y abrir con el código, la copia baja a la
   carpeta de mudanza y la app arranca de nuevo con ella, exactamente como tras una mudanza (`§60`).
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
| El plugin | nombres, tamaños y estados; nunca bytes, tokens ni el código. |
| La WebView de la app | lo mismo que el plugin, más el código de recuperación **una vez**, en pantalla. |

## Lo que falta y lo que está sin probar en dispositivo

- **Cliente OAuth de Google**: hay que darlo de alta (tipo Android, con la SHA-1 de la clave de
  subida y otra de desarrollo) y pasarlo en `FT_GOOGLE_CLIENT_ID` al compilar (va al núcleo con
  `option_env!` y al `AndroidManifest` como esquema de redirección invertido). Sin él, «Conectar
  Google Drive» dice que no hay cliente configurado. También la pantalla de consentimiento
  (verificación de Google, días o semanas).
- **Kotlin/Swift** (`authorize`) sin compilar ni probar aquí: no hay SDK en este entorno. Los
  tests de Rust cubren el flujo con un navegador falso.
- **Subidas largas en segundo plano** (servicio en primer plano en Android): pendiente; hoy una
  subida grande necesita la app en pantalla y, si Android la mata, queda pendiente y se reintenta.
- **Pago** (producto `drive`, fase 6 del plan), Dropbox, iCloud: fuera de esta entrega.
- Pruebas contra Google real (`tests/live_*`): pendientes del cliente.
