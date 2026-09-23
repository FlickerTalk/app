# FlickerTalk: qué hace la app hoy

Estado a 2026-09-23 (versión 1.0.0 en revisión en Google Play; iOS pendiente de la cuenta de
Apple). Describe la app tal como funciona, pantalla a pantalla. Las decisiones de diseño y el porqué
de cada una están en el plan del proyecto; aquí solo lo que el usuario ve y lo que pasa por debajo.

## Qué es

Mensajería privada de teléfono a teléfono. Texto, ficheros, mensajes de voz y llamadas de voz y de
vídeo, siempre entre dos personas. El historial vive **solo en los teléfonos**. Los mensajes viajan
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
  Descargas.
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
- Al sexto dígito, si el PIN es de una sesión existente, se abre; si no, se **crea** una nueva
  vacía. Nada dice cuál de las dos cosas ha pasado: no existe «PIN incorrecto», así que nadie puede
  saber si hay sesiones.
- En **Chats**, cada sesión abierta es un panel plegable bajo la lista principal: una flecha a la
  izquierda y, a la derecha, un botón de **QR** para añadir un contacto a esa sesión y otro para
  **salir**. Ningún texto.
- Un contacto añadido dentro de una sesión pertenece a ella para siempre y nunca aparece en la
  lista principal. Funciona en los dos sentidos: escanear a alguien desde la sesión o que alguien
  escanee el QR que muestra la sesión.
- **Sesión cerrada:** sus mensajes y ficheros llegan y se confirman, pero no hay aviso, ni número de
  no leídos, ni notificación, ni siquiera con la app cerrada. Sus llamadas no suenan (el que llama
  recibe «busy») y no aparecen en Calls. Todo se ve al abrirla con su PIN.
- Salir la cierra en el acto; al reiniciar la app todas están cerradas.
- **Como mucho siete sesiones.** El teléfono registra siempre ocho direcciones en el servidor, se
  usen o no, para que el servidor no sepa cuántas sesiones hay.

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
- Un plugin nunca ve tu identidad, tus claves, tu agenda ni el historial.

## Ajustes

- **Your FlickerTalk ID**, copiar y mostrar tu QR.
- **Offline mailbox:** buzón cifrado, activado por defecto. Apagado, nada tuyo se guarda en el
  servidor y los mensajes esperan en el teléfono del emisor hasta que haya conexión directa.
- **Delivered and read receipts:** valor por defecto para los contactos nuevos.
- **Hours**, **Calls**, **Blocked**, **Session** (arriba).
- **Color** (Mono, Ember, Aurora) y **Appearance** (sistema, oscuro, claro).
- **Plugins**, **Move to a new phone**, **Erase this phone** (quita el dispositivo del servidor y
  borra todo el teléfono, con confirmación).
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
- El buzón: solo mensajes cifrados, sin remitente, que se borran al recogerlos o a los 7 días.
- Sin registros de acceso, sin analítica, sin cuentas.

## Plataformas

Android: publicada en revisión (1.0.0). iOS: la app compila y funciona en un iPhone, pero el push,
las llamadas con la app cerrada y la publicación esperan a la cuenta de pago de Apple.
