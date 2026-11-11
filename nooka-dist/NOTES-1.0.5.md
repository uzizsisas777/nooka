## Qué cambió

**Compartir pantalla en la app nativa.** Hasta ahora, pulsar «Compartir pantalla» dentro de un canal de voz en la app de escritorio no podía funcionar: Electron no trae el selector que sí trae un navegador, así que la petición de pista se rechazaba con `NotAllowedError` y no pasaba nada visible. La app abre ahora su propio selector con las pantallas y las ventanas reales del sistema —miniatura de cada una, dos pestañas (Pantallas / Ventanas) y la etiqueta de cada pantalla con su resolución, marcando la principal—, y lo que se elige es lo único que se entrega.

**Sin elección no hay stream.** El envoltorio no entrega ninguna pista si nadie eligió una fuente en el selector, así que el escritorio nunca se emite solo. Cancelar sale sin tocar nada: una transmisión que estuviera delante sigue como estaba. Si la ventana elegida se cerró mientras el selector estaba abierto, el selector lo dice y vuelve a leer la lista.

**Reparto del trabajo.** Este paquete trae la mitad del sistema (el proceso principal atiende la petición de captura y publica la lista de fuentes por el puente). La mitad que pinta el selector vive en la web desplegada (`nooka-web.vercel.app`), que es la que carga la app, así que el selector aparece en cuanto esa web tenga el cambio —no hace falta esperar otro instalador para ello—.

## Nota

La firma Authenticode sigue pendiente: es la única cura estructural que queda para los veredictos por hash y no hay ruta a coste cero (Azure Trusted Signing pide tarjeta y 9,99 $/mes; SignPath exige licencia OSI y código ajeno).
