const { app, BrowserWindow, ipcMain, shell } = require('electron');
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