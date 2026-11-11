const { contextBridge, ipcRenderer } = require('electron');

// `sendSync` en vez de `invoke`: el puente tiene que estar publicado antes del PRIMER script de la
// página. Con `invoke` el cliente leería `window.NookaNative` vacío en la carga de su módulo y
// arrancaría creyéndose web. Aquí no se toca el DOM: con `sandbox: true` el preload corre en
// `document_start`, donde `document.documentElement` todavía es null, y un `classList` ahí lanza y
// se lleva por delante el `exposeInMainWorld` de abajo (medido: `hasBridge: false`). La marca
// visual la pone el cliente, sobre su propio elemento, cuando ese elemento ya existe.
const env = ipcRenderer.sendSync('nooka:env') || {};

contextBridge.exposeInMainWorld('NookaNative', {
  isDesktop: true,
  platform: env.platform ?? null,
  version: env.version ?? null,
  packaged: env.packaged === true,
  serverUrl: env.serverUrl ?? null,
  window: {
    minimize: () => ipcRenderer.send('nooka:window:minimize'),
    maximize: () => ipcRenderer.send('nooka:window:maximize'),
    close: () => ipcRenderer.send('nooka:window:close'),
    isMaximized: () => ipcRenderer.invoke('nooka:window:isMaximized'),
  },
  // Compartir pantalla. El navegador tiene su propio selector; Electron no, así que el cliente pinta
  // el suyo con estas dos llamadas y después llama a `getDisplayMedia()` normal, que el proceso
  // principal atiende con la elección guardada. `choose` responde `false` si la fuente ya no existe
  // (la ventana elegida se cerró mientras el modal estaba abierto).
  capture: {
    sources: () => ipcRenderer.invoke('nooka:capture:sources'),
    choose: (sourceId) => ipcRenderer.invoke('nooka:capture:choose', sourceId),
  },
});
