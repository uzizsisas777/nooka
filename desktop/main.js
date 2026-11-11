const { app, BrowserWindow, ipcMain, shell, desktopCapturer, screen, session } = require('electron');
const path = require('path');

const isDev = !app.isPackaged;
const VITE_DEV_URL = 'http://localhost:5173';
const PRODUCTION_URL = 'https://nooka-web.vercel.app';
const APP_URL = isDev ? VITE_DEV_URL : PRODUCTION_URL;
// El server con el que habla el cliente nativo. En dev es el de siempre, en 127.0.0.1:3001; en el
// paquete se deja elegir con NOOKA_SERVER_URL y, si no se fija, vale NULO: la app carga la web
// desplegada y ésa ya trae su propio origen, así que adivinar uno aquí rompería más de lo que arregla.
const SERVER_URL = process.env.NOOKA_SERVER_URL ?? (isDev ? 'http://127.0.0.1:3001' : null);
const APP_ICON = path.join(__dirname, 'assets', 'icon.ico');

// El AUMID es lo que hace que la barra de tareas y los avisos de Windows agrupen bajo Nooka con
// SU icono; sin él, la ventana se presenta como «Electron» y el aviso sale con el icono genérico.
app.setAppUserModelId('com.nooka.desktop');

// Como Discord: un solo Nooka por sesión. El segundo clic enfoca la ventana existente
// y ese proceso nuevo se va; el splash del launcher sí aparece otra vez porque Squirrel
// tampoco bloquea Update.exe.
const gotSingleInstanceLock = app.requestSingleInstanceLock();
if (!gotSingleInstanceLock) {
  app.quit();
}

let mainWindow = null;
let splashWindow = null;

function createSplash() {
  splashWindow = new BrowserWindow({
    width: 420,
    height: 320,
    frame: false,
    resizable: false,
    center: true,
    show: true,
    alwaysOnTop: true,
    transparent: false,
    backgroundColor: '#313338',
    icon: APP_ICON,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false, // needed for require in splash.html script
    },
  });
  splashWindow.loadFile(path.join(__dirname, 'splash.html'));
  splashWindow.on('closed', () => { splashWindow = null; });
}

function createMainWindow() {
  mainWindow = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 940,
    minHeight: 560,
    backgroundColor: '#131416',
    title: 'Nooka',
    show: false,
    frame: false,
    icon: APP_ICON,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      webSecurity: true,
    },
    autoHideMenuBar: true,
  });

  const url = APP_URL;

  mainWindow.webContents.on('did-finish-load', () => {
    console.log('[main] did-finish-load');
    // Swap splash -> main once renderer is ready
    if (splashWindow) {
      splashWindow.close();
      splashWindow = null;
    }
    mainWindow.show();
    if (isDev) {
      mainWindow.webContents.openDevTools({ mode: 'detach' });
      // En modo dev, limpiar el storage si hay un token inválido
      mainWindow.webContents.executeJavaScript(`
        (async () => {
          const token = localStorage.getItem('nooka_token');
          if (token) {
            try {
              const res = await fetch('${SERVER_URL}/api/v1/users/@me', {
                headers: { 'Authorization': 'Bearer ' + token }
              });
              if (res.status === 401) {
                console.log('[main] Token inválido, limpiando storage...');
                localStorage.clear();
                location.reload();
              }
            } catch (e) {
              console.error('[main] Error al verificar token:', e);
            }
          }
        })();
      `).catch(() => {});
    }
  });

  // Atajo Ctrl+Shift+R para resetear la app en modo dev
  if (isDev) {
    mainWindow.webContents.on('before-input-event', (event, input) => {
      if (input.control && input.shift && input.key === 'R') {
        console.log('[main] Reset manual triggered');
        mainWindow.webContents.executeJavaScript(`
          localStorage.clear();
          sessionStorage.clear();
          location.reload();
        `).catch(() => {});
        event.preventDefault();
      }
    });
  }

  mainWindow.webContents.on('did-fail-load', (event, errorCode, errorDescription) => {
    console.error('[main] did-fail-load:', errorCode, errorDescription);
  });

  mainWindow.webContents.on('crashed', () => {
    console.error('[main] renderer crashed');
  });

  mainWindow.webContents.on('render-process-gone', (event, details) => {
    console.error('[main] render-process-gone:', details);
  });

  mainWindow.loadURL(url).catch((err) => {
    console.error('[main] loadURL failed:', err);
  });

  const origins = [APP_URL, SERVER_URL].filter(Boolean);
  const isOwn = (u) => origins.some((o) => u === o || u.startsWith(o + '/'));

  // Un `<a href>` normal NO pasa por `setWindowOpenHandler`: navega la ventana y el usuario se
  // queda con la app sustituida por la página de otro. Ésa es la ruta por la que sale casi todo
  // (los enlaces de los mensajes sin `target`), así que el corte va aquí además de abajo.
  mainWindow.webContents.on('will-navigate', (event, target) => {
    if (isOwn(target)) return;
    event.preventDefault();
    shell.openExternal(target);
  });

  mainWindow.webContents.setWindowOpenHandler(({ url: u }) => {
    if (u.startsWith('http') && !isOwn(u)) {
      shell.openExternal(u);
      return { action: 'deny' };
    }
    return { action: 'allow' };
  });

  mainWindow.on('closed', () => { mainWindow = null; });
}

app.on('second-instance', () => {
  if (mainWindow) {
    if (mainWindow.isMinimized()) mainWindow.restore();
    if (!mainWindow.isVisible()) mainWindow.show();
    mainWindow.focus();
  }
});

app.whenReady().then(() => {
  if (!gotSingleInstanceLock) return;
  installDisplayMediaHandler();
  createSplash();
  // Small delay so splash paints before heavy load
  setTimeout(() => createMainWindow(), 300);
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createMainWindow();
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});

// `on` + `returnValue` en vez de `handle`: es lo que permite al preload leerlo con `sendSync` y
// publicar el puente completo antes del primer script de la página. Un `invoke` sería asíncrono y
// el cliente nacería creyéndose web.
ipcMain.on('nooka:env', (event) => {
  event.returnValue = {
    platform: process.platform,
    version: app.getVersion(),
    packaged: app.isPackaged,
    serverUrl: SERVER_URL,
  };
});
ipcMain.on('nooka:window:minimize', () => { if (mainWindow) mainWindow.minimize(); });
ipcMain.on('nooka:window:maximize', () => { if (mainWindow) (mainWindow.isMaximized() ? mainWindow.unmaximize() : mainWindow.maximize()); });
ipcMain.on('nooka:window:close', () => { if (mainWindow) mainWindow.close(); });
ipcMain.handle('nooka:window:isMaximized', () => (mainWindow ? mainWindow.isMaximized() : false));

// ─── Compartir pantalla ───────────────────────────────────────────────────────────────────────
// Electron no trae selector de captura: sin handler, `navigator.mediaDevices.getDisplayMedia()`
// rechaza con `NotAllowedError` y el botón «Compartir pantalla» no abre nada (así estaba hasta
// 2026-11-11). El reparto de papeles es el inverso del de un navegador —allí el picker es del
// Chrome; aquí el proceso principal sólo sabe QUÉ hay en pantalla y el renderer lo pinta, en
// `web/src/components/dialogs/ScreenShareSourceModal.tsx`—. Entre los dos no viaja una pista,
// viaja un id.
const THUMBNAIL_SIZE = { width: 256, height: 144 };

// La última lista, conservada entera: `Streams.video` quiere una fuente de `getSources`, no un id
// crudo (el paquete 1.0.4 corre Electron 28, y ahí la forma documentada es el objeto).
let lastSources = new Map();

// Una sola elección pendiente. No hay dos ventanas que puedan compartir: el splash no pregunta y el
// DevTools no corre la app.
let pendingSourceId = null;

// «Entire Screen 0» no le dice nada a nadie. Los monitores se rotulan con su tamaño y se marca el
// principal, que es lo que Windows enseña en su propio selector. `display.size` viene en píxeles
// lógicos (ya incluye el escalado), que es la cifra que ve el usuario en Configuración de pantalla.
function displayLabels() {
  const primary = String(screen.getPrimaryDisplay().id);
  const labels = new Map();
  screen.getAllDisplays().forEach((display, index) => {
    labels.set(String(display.id), {
      name: `Pantalla ${index + 1} · ${display.size.width}×${display.size.height}`,
      primary: String(display.id) === primary
    });
  });
  return labels;
}

async function captureSources() {
  const sources = await desktopCapturer.getSources({
    types: ['screen', 'window'],
    thumbnailSize: THUMBNAIL_SIZE
  });
  const labels = displayLabels();
  lastSources = new Map();
  const rows = [];
  for (const source of sources) {
    const isScreen = source.id.startsWith('screen:');
    // Una ventana minimizada o de un proceso ya muerto devuelve miniatura vacía y su captura sale
    // congelada: en el selector del navegador esas filas existen y se pueden elegir; aquí no.
    if (!isScreen && source.thumbnail.isEmpty()) continue;
    const label = isScreen ? labels.get(String(source.display_id)) : null;
    lastSources.set(source.id, source);
    rows.push({
      id: source.id,
      kind: isScreen ? 'screen' : 'window',
      name: label ? label.name : source.name,
      primary: label ? label.primary : false,
      thumbnail: source.thumbnail.isEmpty() ? null : source.thumbnail.toDataURL()
    });
  }
  return rows;
}

// El `audioRequested` se ignora a propósito: la malla WebRTC no manda audio del sistema y
// `web/src/voice.ts` no lo pide, así que darlo sería inventarse una pista que nadie consume.
function installDisplayMediaHandler() {
  session.defaultSession.setDisplayMediaRequestHandler((request, callback) => {
    const chosenId = pendingSourceId;
    // Se consume SIEMPRE, se atienda o no: una elección que sobreviva sería la que se comparta la
    // próxima vez sin que nadie la pida.
    pendingSourceId = null;
    const chosen = chosenId ? lastSources.get(chosenId) : null;
    // Sin elección no se comparte nada. Autoelegir el primer monitor sería lanzar el escritorio
    // entero a una llamada que nadie autorizó: el cliente viejo (el de la web desplegada, que aún no
    // tiene modal) se queda con su aviso de error, que es verdad.
    if (chosen) {
      callback({ video: chosen });
      return;
    }
    // Denegar ES llamar al callback sin `video`, y Electron responde a eso con un `ThrowTypeError`
    // SINCRONO (`shell/browser/electron_browser_context.cc`, rama `(video_requested && !has_video)`).
    // Sin el `try` la excepción escapa del handler, rechaza el `async` que Electron pone por delante en
    // `lib/browser/api/session.ts` y sale en consola como `UnhandledPromiseRejectionWarning`; el
    // `AbortError` del renderer ya se lanzó a los 7 ms de medirlo.
    try {
      callback({});
    } catch {
      // Es la denegación, no un fallo nuestro: `web/src/voice.ts` muestra su aviso.
    }
  });
}

ipcMain.handle('nooka:capture:sources', () => captureSources());

// Devuelve si la fuente sigue viva: entre que se pinta la lista y se pulsa «Compartir» la ventana
// elegida se puede haber cerrado, y entonces el selector se relee en vez de abrir una captura falsa.
// No se guarda nada cuando el id no está en la última lista: una elección muerta no puede colarse en
// la próxima petición.
ipcMain.handle('nooka:capture:choose', (_event, sourceId) => {
  const live = typeof sourceId === 'string' && sourceId !== '' && lastSources.has(sourceId);
  pendingSourceId = live ? sourceId : null;
  return live;
});
