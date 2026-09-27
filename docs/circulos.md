# Círculos: diseño

Decisión del 2026-09-27. Implementado en `crates/ft-circles`, `crates/ft-core/src/circles.rs`,
`src-tauri` (`core_circle_*`) y `src/` (`CircleThread`, `CircleInfoPage`, `NewCirclePage`).

## Qué es

Un **círculo** es un grupo pequeño y cerrado de contactos (32 como mucho) **sin que el servidor
sepa que existe**. Un «canal» (uno escribe, muchos leen) no es otra cosa: es un círculo con el
interruptor «solo escriben los administradores». Los canales públicos, a los que cualquiera se
suscribe sin conocer a nadie, **no** encajan: exigen que el servidor guarde la lista de suscriptores
y reparta él, y entonces sabe quién sigue qué. Se descartan.

## Cómo funciona

- **La tarjeta** (`ft-circles`): id aleatorio, revisión, nombre, administradores, miembros (cada uno
  con su Contact Card firmada) y `admins_only`, firmada con la Ed25519 de un administrador. Viaja
  cifrada de extremo a extremo como cualquier paquete. Un teléfono sustituye la que tiene solo por
  una revisión mayor firmada por un administrador **de la que tiene** (cadena); un empate lo gana
  el id de dispositivo más bajo. Un círculo nuevo se acepta solo de un contacto aceptado y solo si
  lo firma uno de sus propios administradores.
- **Envío en abanico**: un texto se guarda una vez y se pone en `circle_outbox` una vez por
  miembro. Cada entrada se entrega como un mensaje 1 a 1: directo si hay canal, buzón si los dos
  lo tienen, si no espera y reintenta. El router ve n sobres sellados indistinguibles de n
  mensajes 1 a 1: ni el id del círculo, ni el emisor, ni el nombre (probado en
  `tests/circles.rs`). El acuse de cada miembro limpia su entrada; «entregado» cuando no falta
  nadie. Sin acuses de lectura en círculos: revelarían a n personas cuándo miras el teléfono.
- **Conocerse por la tarjeta**: al adoptar una tarjeta, cada miembro desconocido se guarda como
  contacto de círculo (`via_circle`, sin aceptar) con un canal Olm abierto desde su Contact Card.
  No sale en la lista ni en Solicitudes ni puede llamar; si escribe 1 a 1, es una solicitud (A5).
  Cuando ya no comparte ningún círculo y no ha escrito, se olvida.
- **Pertenencia**: crear (el creador es administrador), invitar, expulsar (el expulsado recibe la
  tarjeta nueva y se entera; los demás dejan de enviarle), nombrar y quitar administradores (el
  último no puede dejar de serlo), renombrar, «solo administradores», salir (`CircleLeave` a
  todos; la siguiente revisión de un administrador lo consolida). Sin administradores el círculo
  queda congelado: se lee y se puede salir, nadie cambia la tarjeta.
- **Historial**: quien entra después no ve lo dicho antes. El orden es el de llegada a cada
  teléfono (`received_at`), como en los chats.
- **Sesiones ocultas**: un círculo creado en una sesión es de sus contactos, vive con ella y calla
  cuando la sesión está cerrada.

## Por qué así y no con Megolm o MLS

Con 32 miembros, cifrar n veces son milisegundos y las n copias hay que mandarlas de todos modos,
porque el router no reparte. A cambio: cero claves de grupo que rotar cuando alguien se va,
secreto hacia delante por pareja (el de Olm), compatibilidad con el buzón y con las sesiones
ocultas, **nada que cambiar en el servidor** y tests de tres dispositivos sobre la red en memoria
que ya existía. Megolm compensa por encima de ~50 miembros y queda acotado a `CircleMessage`
(clave de grupo por emisor repartida por Olm, cifrar una vez, el mismo cifrado a todos). MLS
exige que alguien ordene los commits, es decir, un servidor que sabe del grupo: no.

## Lo que no está (y por qué)

- **Ficheros en círculos**: solo por conexión directa, n transferencias por fichero (o reparto
  entre miembros que ya lo tienen). Segunda fase.
- **Llamadas en círculos**: una malla P2P no aguanta en un móvil a partir de 4 y un SFU es un
  servidor que ve quién habla con quién. Producto aparte, si algún día.
- **Huecos en la secuencia** («faltan mensajes de Ana»): un contador por emisor y un
  `CircleResend`. Opcional; de momento los reintentos no paran hasta el acuse.
- **Retención por círculo** (las reglas «keep history» y «delete after reading» son por contacto).
