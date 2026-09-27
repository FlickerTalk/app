# FlickerTalk: qué hace la app hoy

Estado a 2026-09-27 (versión 1.1.0 sin publicar; la 1.0.0 en revisión en Google Play; iOS
pendiente de la cuenta de Apple; incluye el paquete de endurecimiento de la revisión del
2026-09-24, los círculos, los plugins de fase 3 y la nube del usuario). Describe la app tal como funciona, pantalla a pantalla. Las decisiones de diseño y el porqué
de cada una están en el plan del proyecto; aquí solo lo que el usuario ve y lo que pasa por debajo.

## Qué es

Mensajería privada de teléfono a teléfono. Texto, ficheros, mensajes de voz y llamadas de voz y de
vídeo entre dos personas, y **círculos** de texto entre unas pocas. El historial vive **solo en
los teléfonos**. Los mensajes viajan
cifrados de extremo a extremo (Olm, la biblioteca `vodozemac`) por una conexión directa WebRTC
entre los dos teléfonos. Si el otro teléfono no está disponible, el mensaje espera cifrado en un
buzón del servidor hasta que lo recoge (como mucho 7 días). El servidor no puede leer nada y no
guarda historial.

La interfaz va con iconos primero. Todo texto visible sale de un catálogo de traducciones con el
inglés como fuente; la traducción al español está en curso.

## Primer arranque

- **Bienvenida:** la identidad se crea en el teléfono al abrir la app. No hay cuenta, ni número de
  teléfono, ni correo. Solo se pide un nombre, opcional, que ven los contactos al añadirte.
- **«I have FlickerTalk on another phone»:** en lugar de crear una identidad nueva, trae la del
  teléfono viejo (ver «Cambiar de teléfono»).
- La app pide permiso de notificaciones y, cuando hace falta, el de micrófono y cámara.

## Añadir contactos

Pantalla **Add contact** (icono de QR en Chats):

- **My code:** el QR de tu tarjeta de contacto firmada. El otro lo escanea y quedáis emparejados.
- **Share link:** manda el mismo enlace por cualquier app (WhatsApp, correo…).
- **Scan:** la cámara lee el QR del otro. También se puede **pegar su enlace**.

No hay agenda ni búsqueda por teléfono: solo te encuentra quien tiene tu código.

- **Solicitudes:** quien te escribe primero con tu enlace, sin que tú lo hayas escaneado, aparece
  en **Solicitudes**, encima de la lista, con su nombre, su identificador corto y lo que ha dicho.
  Hasta que aceptes no se descarga ningún fichero suyo, sus llamadas reciben «busy» y no suena
  nada. Aceptar lo pasa a la lista; rechazar lo bloquea. Escanearlo tú, o contestarle, también es
  aceptar.
- **Renovar mi enlace** (Ajustes): retira el enlace actual y hace uno nuevo. Quien guardara el
  antiguo ya no puede llegar a ti; tus contactos reciben la tarjeta nueva solos. Cada sesión
  oculta puede renovar el suyo por separado.

## Chats

- Lista de conversaciones con avatar, último mensaje, hora, estado y número de no leídos. El punto
  del avatar indica conexión directa abierta.
- En pantallas anchas (tablet) la conversación se abre al lado de la lista; en el móvil, a pantalla
  completa.
- El icono **⋮** de cada fila abre la ficha del contacto.

## Conversación

- **Texto y emoji** (selector propio con recientes y categorías).
- **Ficheros** (clip): van solo por conexión directa, en trozos y con reanudación; nunca por el
  buzón. Imágenes y audio se ven dentro del chat; el resto se abre con otra app o se guarda en
  Descargas. Los que superan el tamaño de **Descargar archivos solos** (Ajustes; 10 MB por
  defecto) esperan con su tamaño y un botón: no llega ni un byte hasta que lo tocas. Una oferta
  de más de 2 GB se rechaza sin más.
- **Mensajes de voz:** mantener pulsado el micrófono, soltar para enviar o descartar. Viajan como
  un fichero.
- **Estados de cada mensaje:** esperando al otro teléfono, enviado, entregado, leído. Nunca se marca
  como entregado algo que no lo está.
- **Pulsación larga en un mensaje:** plegar, reenviar a otro contacto, compartir con otra app o
  borrar de este teléfono.
- **Herramientas (plugins):** el botón de apps del chat abre las instaladas (ver «Plugins»).
- **Llamada de voz y de vídeo** desde la cabecera.

## Llamadas

- Voz y vídeo 1 a 1, por WebRTC. La llamada entrante suena con el tono del teléfono y aparece como
  notificación de llamada con **Answer** y **Decline**, también con la app cerrada o la pantalla
  bloqueada. Contestar desde la notificación abre la pantalla de llamada.
- En llamada: silenciar micrófono, altavoz, cámara y colgar.
- Una llamada a la vez: si ya estás en una, el otro recibe «busy».
- Pestaña **Calls:** historial (entrantes, salientes, perdidas) con botón de devolver la llamada.
- Ajustes → **Calls:** «Only direct» (nunca por el relay), «Relay when needed» o «Always relay»
  (oculta tu IP al otro).

## Ficha del contacto

Desde ⋮ en la lista o en la cabecera del chat. Todo lo de aquí es de **este teléfono**: el contacto
no se entera y el servidor no lo ve.

- **Nombre** con el que aparece en este teléfono.
- **Security fingerprint:** doce grupos que los dos teléfonos calculan igual; se comparan en
  persona para confirmar que nadie se ha metido en medio.
- **Keep history:** siempre, 30 días, 7 días o 1 día.
- **Delete after reading:** los mensajes leídos se borran al cabo de 1 min, 5 min o 1 h.
- **Mute:** sus llamadas aparecen en pantalla pero no suenan ni vibran.
- **Messages:** apagado, sus mensajes y ficheros no se guardan; él solo ve «enviado».
- **Calls:** apagado, sus llamadas no entran; él recibe «busy» y aquí no queda rastro.
- **Delivered and read receipts:** apagado, él nunca ve «entregado» ni «leído» (sus mensajes se
  quedan en «enviado»), aunque sí deja de reintentar.
- **Block:** corta la conexión y descarta todo lo suyo. Se deshace en Ajustes → Blocked.
- **Report:** abre un correo a FlickerTalk con el motivo y, si quieres, sus últimos mensajes como
  prueba; además lo bloquea.

## Sesiones ocultas

Espacios con sus propios contactos y conversaciones que **solo conoce quien los crea**. Sirven para
separar vidas: un chat con los amigos aparte, o uno de trabajo que se cierra al salir de la
oficina y no molesta hasta el día siguiente.

- **Ajustes → Session** abre un teclado numérico con seis puntos. Sin nombre, sin título.
- Al sexto dígito, si el PIN es de una sesión existente, se abre; si no, se crea una sesión
  **nueva y vacía**. Todo PIN es válido: no existe «PIN incorrecto» ni espera entre intentos, y
  nada dice cuál de las dos cosas ha pasado, así que nadie puede saber si hay sesiones.
- Una sesión **vacía** (sin contactos, solicitudes ni círculos) se borra al salir de ella, o al arrancar si
  la app se cerró con ella abierta: teclear PIN nunca llena los siete huecos. Con algo dentro, se
  queda.
- Una sesión que se borra se lleva su enlace: quien guardara su QR ya no llega a nadie, ni a la
  sesión que ocupe después ese hueco ni a la lista principal.
- En **Chats**, cada sesión abierta es un panel plegable bajo la lista principal: una flecha a la
  izquierda y, a la derecha, un botón de **QR** para añadir un contacto a esa sesión, una
  **papelera** para borrarla (pide confirmación en el mismo sitio; borra sus contactos, historial
  y ficheros, y libera su hueco) y otro para **salir**. Ningún texto.
- Un contacto añadido dentro de una sesión pertenece a ella para siempre y nunca aparece en la
  lista principal. Funciona en los dos sentidos: escanear a alguien desde la sesión o que alguien
  escanee el QR que muestra la sesión.
- **Sesión cerrada:** sus mensajes y ficheros llegan y se confirman, pero no hay aviso, ni número de
  no leídos, ni notificación, ni siquiera con la app cerrada. Sus llamadas no suenan (el que llama
  recibe «busy») y no aparecen en Calls. Todo se ve al abrirla con su PIN.
- Salir la cierra en el acto; al reiniciar la app todas están cerradas.
- **Como mucho siete sesiones.** El teléfono registra siempre ocho direcciones en el servidor, se
  usen o no, para que el servidor no sepa cuántas sesiones hay.

## Círculos

Un círculo es un grupo **pequeño y cerrado** de tus contactos (hasta 32): los amigos, la familia,
el trabajo. Nadie fuera de él, **ni siquiera el servidor**, sabe que existe: cada mensaje se cifra
y se envía a cada miembro por el mismo canal que ya usas con él, directo o por su buzón, así que el
servidor ve mensajes 1 a 1 y nada más.

- **Crear:** el icono de personas en Chats (o en la cabecera de una sesión oculta) pide un nombre y
  a quién meter, de entre tus contactos. Quien lo crea es su administrador.
- **Conocer a los demás sin escanear a nadie:** la tarjeta del círculo lleva la tarjeta de contacto
  de cada miembro, así que todos pueden escribirse dentro del círculo. Fuera de él, alguien a quien
  solo conoces por un círculo **no** aparece en tu lista ni puede llamarte; si te escribe por su
  cuenta, entra en Solicitudes como cualquier desconocido.
- **Dentro:** cada mensaje dice quién lo dijo; lo que pasa (quién entró, quién salió, cambios de
  nombre) sale como una línea. Quien entra después no ve lo dicho antes. Los estados son los de
  siempre: «entregado» cuando ha llegado a todos. En círculos no se envían acuses de lectura.
- **Ajustes del círculo** (⋮ en la fila o tocando la cabecera): miembros, añadir a otro contacto,
  expulsar, nombrar o quitar administradores, cambiar el nombre y **«solo escriben los
  administradores»** (un boletín: los demás leen). Solo los administradores cambian estas cosas;
  todo lo irreversible pregunta una vez en el sitio.
- **Salir** avisa a todos; lo dicho se queda en el teléfono en solo lectura hasta que borras el
  círculo. Si te expulsan, te enteras y el círculo queda igual, en solo lectura.
- Solo texto por ahora: ni ficheros, ni notas de voz, ni llamadas en círculos.

## Horario

**Ajustes → Hours.** Apagado por defecto. Encendido, cada día de la semana es **todo el día**,
**nunca** o un **tramo** de horas (puede pasar de medianoche). Fuera del horario los mensajes llegan
sin notificación y las llamadas se ven sin sonar. Lo comprueba el teléfono con su propio reloj,
también con la app cerrada; no sale del teléfono.

## Plugins

**Ajustes → Plugins.** Ninguna herramienta viene dentro de la app salvo cinco pequeñas de serie:
imágenes, PDF, tapar datos, dibujo y markdown. El resto se instala desde un catálogo firmado.

- Cada plugin corre aislado y empieza sin ningún permiso. El usuario decide qué puede hacer y lo
  puede quitar: leer lo que tú le entregas, escribir en el chat, enviar mensajes por su cuenta.
  Sin «escribir en el chat», nada de lo que haga llega a la conversación; con él, lo que haga (un
  texto o un fichero) queda en el compositor y lo envías tú; solo con «enviar por su cuenta» sale
  solo.
- Un plugin nunca ve tu identidad, tus claves, tu agenda ni el historial.
- **Desde el 2026-09-27** (versión 1.1.0): un plugin puede pedir, y tú conceder aparte, **hablar
  con el mismo plugin al otro lado** de la conversación (solo por la conexión directa, nunca por
  el servidor), **poner avisos** en tu teléfono, **usar tu nube** y **guardar muchos datos** en el
  teléfono (256 MB). En la pulsación larga de una burbuja aparece **«Abrir con»** para los plugins
  que abren ese tipo de fichero o un texto. Un plugin se puede abrir solo desde Ajustes → Plugins.
- **Notas**: notas personales con un aviso (una notificación en tu teléfono a la hora que
  elijas, una sola vez). Nacen desde un mensaje («Abrir con» → Notas: llega el texto y un camino
  de vuelta a la conversación, nunca el contacto) o desde cero. Por defecto la notificación solo
  dice que tienes un aviso; el texto de la nota se enseña si lo activas.
- **Pizarra**: lienzo infinito con trazo a mano, texto, fórmulas (LaTeX o con paleta de símbolos)
  e imágenes; deshacer y rehacer; se guarda o se envía como `.ftboard`, se abre en solo lectura al
  otro lado y se reproduce trazo a trazo. **En directo**: en una conversación donde los dos tienen
  el plugin abierto, lo que dibuja uno aparece en el otro al momento, por la conexión directa.
- **Mi drive**: ver «Copia de seguridad y Mi drive».

## Copia de seguridad y Mi drive

Desde el 2026-09-27 (`docs/drive.md`). Una copia sellada de tu teléfono, y tus ficheros, en **tu
propio Google Drive**. Todo se cifra en el teléfono antes de salir: Google ve ficheros sellados con
nombres aleatorios, su tamaño y sus fechas; nuestro servidor no participa.

- **Ajustes → Copia de seguridad**: conectar Google Drive (el login se abre en el navegador del
  sistema; la app no ve los tokens), crear el drive (se enseña **una sola vez** un código de
  recuperación de 30 símbolos: apúntalo), hacer copia (historial, clave y ficheros), restaurar en
  un teléfono nuevo (conectar la misma nube + código; la app reinicia con la copia, como tras una
  mudanza) y olvidar la nube en este teléfono (los ficheros siguen en tu nube, sellados).
- **Mi drive** (plugin, permiso «usar tu nube»): carpetas, subir del selector, guardar cualquier
  fichero de una burbuja («Abrir con» → Mi drive; del tamaño que sea, los bytes no pasan por el
  plugin), abrir, guardar en Descargas y enviar a la conversación. Lo que espera red se ve con su
  motivo; nunca se marca como guardado lo que no subió.
- Pendiente para usarlo de verdad: el cliente OAuth de Google de la app (`docs/drive.md`).

## Ajustes

- **Your FlickerTalk ID**, copiar y mostrar tu QR.
- **Offline mailbox:** buzón cifrado, activado por defecto. Apagado, nada tuyo se guarda en el
  servidor y los mensajes esperan en el teléfono del emisor hasta que haya conexión directa.
- **Delivered and read receipts:** valor por defecto para los contactos nuevos.
- **Hours**, **Calls**, **Blocked**, **Session** (arriba).
- **Color** (Mono, Ember, Aurora) y **Appearance** (sistema, oscuro, claro).
- **Plugins**, **Move to a new phone**, **Backup** (la nube del usuario), **Erase this phone**
  (quita el dispositivo del servidor y borra todo el teléfono, con confirmación).
- **Plan** y **Version**.

## Plan y precio

1 € al año. El **primer año es gratis** desde la instalación, contado en el propio teléfono, sin
tarjeta. **Menores de 21, siempre gratis**: la edad se declara en el teléfono y nunca sale de él.
Sin pagar se sigue **recibiendo y respondiendo** siempre; lo que no se puede es empezar una
conversación nueva, llamar ni enviar ficheros. La compra dentro de la app está pendiente de dar de
alta el producto en Google Play.

## Cambiar de teléfono

En el teléfono nuevo, «I have FlickerTalk on another phone» muestra un QR; en el viejo, Ajustes →
Move to a new phone lo escanea. La identidad, los contactos y los mensajes pasan directamente de un
teléfono al otro, cifrados, sin pasar por el servidor. El viejo se borra después. Los ficheros
enviados o recibidos no se mueven todavía.

## Lo que guarda el servidor

- Por dispositivo: su identificador, su clave pública, los hashes de sus ocho direcciones de
  entrega y dónde despertarlo (el token de push, cifrado con una clave que no está en la base de
  datos).
- El buzón: solo mensajes cifrados, metidos además en un sobre sellado para el destinatario
  (sin remitente, sin clave del remitente y sin tamaño reconocible: van rellenados a bloques), que
  se borran al recogerlos o a los 7 días. Las señales para conectar van en el mismo sobre.
- Sin registros de acceso, sin analítica, sin cuentas.

## Plataformas

Android: publicada en revisión (1.0.0). iOS: la app compila y funciona en un iPhone, pero el push,
las llamadas con la app cerrada y la publicación esperan a la cuenta de pago de Apple.
