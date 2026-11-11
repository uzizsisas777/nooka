## Qué cambió

**Antivirus.** Este release sale sin `powershell.exe` dentro. El instalador y el actualizador ya no invocan PowerShell para descargar, descomprimir ni crear el atajo: usan `curl.exe` y `tar.exe` (los que trae Windows en System32) y la API `IShellLinkW` por COM. Eso quita justo el comportamiento que los motores leen como "dropper". Además los tres ejecutables por fin tienen identidad de PE: VERSIONINFO (Nooka, FileVersion 1.0.4.0), manifiesto y su icono, así que al pulsar el instalador ves un icono propio y el Explorador ya no ve un archivo anónimo sin marca ni versión. El subsistema es GUI, no consola: la ventanita negra que parpadeaba al actualizar desaparece.

Nada de esto es malware y no lo ha sido: si un motor sigue banderando el archivo, es un falso positivo y su envío está descrito en `desktop/README.md`, §14.1.

**Enlaces.** Un enlace normal dentro del chat ya no sustituye la ventana de Nooka por la página de otro: se abre en tu navegador de siempre. Aplica también a los enlaces que abren ventana nueva.

**Servidor.** El paquete elige el servidor con `NOOKA_SERVER_URL`; si no se fija, la app carga la web desplegada y usa su propio origen (antes se hardcodeaba `127.0.0.1:3001`, que en una máquina sin server local dejaba la app en blanco).

**Arranque y diagnóstico.** El puente del preload es síncrono, así que la web nace sabiendo que es la app nativa en lugar de enterarse un instante después. Se registran `did-fail-load`, fallos del renderer y caídas de proceso. En modo dev, un token inválido se limpia solo al abrir y `Ctrl+Shift+R` resetea la app.

## Nota

La firma Authenticode sigue pendiente: es la única cura estructural que queda para los veredictos por hash y no hay ruta a coste cero (Azure Trusted Signing pide tarjeta y 9,99 $/mes; SignPath exige licencia OSI y código ajeno).
