# app/crates/ft-vault

La nube del propio usuario (plan del drive, fase A, 2026-09-27): un **drive** de ficheros y la
**copia de seguridad** del teléfono (`§61`), sellados en el teléfono antes de que nada salga. El
proveedor (Google Drive hoy) ve blobs sellados con nombres aleatorios, su tamaño y sus fechas;
nuestro servidor no participa (`§100`).

## Formato en la nube

```text
vault.json    versión del formato e id aleatorio del drive, en claro (no dice nada)
key.ftv       la clave del drive, sellada con el código de recuperación
index.ftv     carpetas, ficheros y la copia de seguridad: sellado, uno solo
blob-<id>     un fichero o la base de datos, sellado, con una clave propia
```

- **Cifrado** (`cipher.rs`): XChaCha20-Poly1305 por trozos de 1 MiB con el último marcado (no se
  puede cortar el final ni pegar trozos), cabecera `FTV1` + sal; clave por blob derivada con
  BLAKE3 de la clave del drive + sal + etiqueta (un blob copiado con otro nombre no abre). Es la
  forma de `age` con las primitivas que ya están en el árbol: **`age` no se añadió** (decisión
  2026-09-27: ningún crate nuevo que no estuviera ya) y no hay criptografía casera más allá de
  encadenar AEAD estándar.
- **Código de recuperación** (`recovery.rs`): 30 símbolos de base32 de Crockford (150 bits) que
  genera la app y se enseñan **una vez**; con esa entropía no hace falta estirar (ni scrypt ni
  argon2, que tampoco están en el árbol): la clave que envuelve `key.ftv` sale del código con
  BLAKE3. Al teclearlo se perdonan guiones, espacios, minúsculas y O/I/L.
- **Índice** (`index.rs`): carpetas y ficheros por id aleatorio, con `removed` (lápidas) y
  `revision`. Dos teléfonos que escriben (uno recuperado en otro sitio) se **fusionan** por id:
  gana el cambio más nuevo, una lápida gana a lo más viejo que ella, un fichero cuya carpeta
  desapareció va a la raíz; nunca se pierde un blob subido. Antes de escribir se lee el índice
  remoto y, si cambió, se fusiona.
- **Proveedor** (`provider.rs`): `trait Provider` (leer, escribir, subir, bajar, borrar, listar,
  cuota) con `Memory` para los tests (con interruptor de fallo y corrupción).
- **Google Drive** (`google.rs`): carpeta `FlickerTalk` con el ámbito `drive.file`, ficheros por
  nombre, subida reanudable en partes de 8 MiB, descarga en streaming, cuota; OAuth con PKCE
  (`auth_url`, `code_from_redirect`, `exchange`, `refresh`); los tokens se guardan donde diga el
  `TokenKeeper` (en el núcleo, sellados con la clave de almacenamiento) y se renuevan solos.
  Probado contra un Drive falso (axum) en `tests/google.rs`.
- **Cola** (`lib.rs`): una subida sin red deja el fichero sellado en `outgoing/` y una entrada en
  `queue.json`; el fichero aparece como pendiente **con su motivo** hasta que `run_queue` lo sube.
  Nunca se marca como guardado lo que no subió.
- **Copia de seguridad**: `backup(db, clave, files_dir)` sube la instantánea de la base de datos,
  la clave de almacenamiento (dentro del índice sellado) y los ficheros (los ya subidos con la
  misma ruta y tamaño no se repiten; los blobs de la copia anterior se borran). `restore` deja
  `incoming.db` e `incoming.key` en la carpeta de mudanza, como una mudanza (`§60`): la app los
  cambia al arrancar.

## Reglas

- Sin UI ni plataforma: el login lo abre el puente nativo a través del `Authorizer` del núcleo.
- Nada en claro sale del teléfono: ni nombres, ni contenido, ni la clave, ni el código.
- Tests sin red (`tests/vault.rs`, `Memory`) y contra un servidor local (`tests/google.rs`).
  Pruebas contra Google real: pendientes de un cliente OAuth (`FT_GOOGLE_CLIENT_ID`).
