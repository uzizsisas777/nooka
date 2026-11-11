# Nooka Desktop — build, release y update

Guía de la app de escritorio de Nooka. Todo el pipeline de empaquetado, cómo funciona cada pieza en tiempo de ejecución y cómo probar un candidato antes de subirlo a GitHub.

---

## 1 · Arquitectura en dos líneas

La app de escritorio no es Electron a secas. Son **cuatro binarios** trabajando en cadena, estilo Squirrel.Windows (igual que Discord):

```
NookaSetup.exe  →  Nooka.exe (launcher)  →  Update.exe (updater)  →  app-X.Y.Z/Nooka.exe (Electron)
     Rust              Rust                      Rust                       Electron 42.11.18
```

- **Setup** (`nooka-setup/`) — el instalador que se descarga de GitHub. Embebe el `.nupkg` completo (107 MB), lo descomprime en `%LocalAppData%\Nooka\`, crea los accesos directos y lanza la app por primera vez.
- **Launcher** (`nooka-launcher/`) — `Nooka.exe` en la raíz. Se abre primero desde el acceso directo. Muestra un splash de 1,5 s, pregunta a GitHub si hay versión nueva y, si la hay, invoca al updater. Al terminar arranca el `Nooka.exe` del directorio `app-<versión>` más alto.
- **Updater** (`nooka-updater/`) — `Update.exe` en la raíz. Descarga el `.nupkg` nuevo, lo descomprime a `.staging`, mueve `app-X.Y.N/` al lugar, purga versiones viejas (deja 2) y muestra una ventana de progreso nativa.
- **Electron app** (`desktop/`) — el `main.js` real. Vive dentro de `app-X.Y.Z/Nooka.exe` (Electron renombrado) con su `resources/app.asar` al lado.

Cada capa tiene su propia responsabilidad y su propio log. Nada de esto es mágico.

---

## 2 · Qué instala el setup y dónde

Ruta de instalación fija: `C:\Users\<tú>\AppData\Local\Nooka\`

```
Nooka\
├── Update.exe              ← nooka_updater.exe recompilado (452 KB)
├── Nooka.exe               ← nooka_launcher.exe recompilado (419 KB)
├── icon.ico                ← el icono para el splash y los accesos directos
├── RELEASES                ← manifiesto plano: "SHA1  nooka.X.Y.Z.nupkg  bytes"
├── app-1.0.3\              ← una carpeta por versión instalada
│   ├── Nooka.exe           ← Electron dist renombrado (176 MB, autocontenido)
│   ├── resources\
│   │   └── app.asar        ← el paquete de main.js + preload.js + splash.html + assets
│   ├── *.pak, *.dll, locales, ...  ← todo Electron
├── app-1.0.4\              ← (cuando se actualiza)
├── nooka_launcher.log      ← trazas del launcher
└── SquirrelSetup.log       ← trazas del updater y del setup
```

Accesos directos que crea el setup:
- `%USERPROFILE%\Desktop\Nooka.lnk`
- `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Nooka.lnk`

Ambos apuntan a **exactamente** esto:
```
Target:   %LocalAppData%\Nooka\Update.exe
Args:     --processStart Nooka.exe
Icon:     %LocalAppData%\Nooka\icon.ico
AUMID:    com.nooka.desktop
```

Fíjate: el atajo **no** arranca `Nooka.exe` (el launcher). Arranca `Update.exe --processStart Nooka.exe`. El updater lee `Nooka.exe` en `app-<latest>\` y lo lanza. Si no existe launcher, cae directo al `app-<latest>\Nooka.exe` (Electron).

---

## 3 · Dev mode (probar sin empaquetar)

El launcher, el updater y el setup NO tienen modo dev — son Rust, compilan y ya. La que sí tiene dev mode es la **app Electron**. Cómo correrla:

### 3.1 · Un `package.json` mínimo en `desktop/`

Ahora mismo `desktop/` **no tiene `package.json`** y Electron no puede arrancar sin uno. Créele este (nunca se sube a la release, sólo es para el dev):

```json
{
  "name": "nooka-desktop",
  "version": "1.0.4",
  "main": "main.js"
}
```

Eso es todo. No hace falta `npm install` en `desktop/` porque `node_modules/electron` ya está ahí (42.11.18).

### 3.2 · Levantar el server y el Vite

```bash
# Terminal 1 — el server Nooka en 3001
cd C:/Users/ediss/Documents/APP/Nooka/server
node --experimental-strip-types server/src/index.ts

# Terminal 2 — el Vite del cliente web en 5173
cd C:/Users/ediss/Documents/APP/Nooka/web
npx vite --port 5173 --strictPort --host 127.0.0.1
```

### 3.3 · Correr Electron

```bash
cd C:/Users/ediss/Documents/APP/Nooka/desktop
npx electron .
```

`main.js` hace `isDev = !app.isPackaged` → en dev es `false` → carga `http://localhost:5173` (VITE_DEV_URL), abre DevTools en ventana separada y permite `127.0.0.1:3001` para el gateway. En producción carga `https://nooka-web.vercel.app`.

### 3.4 · Trucos de dev

- **Cambiar el `PRODUCTION_URL` sin recompilar**: no hay env var todavía; edita `desktop/main.js:5`.
- **Probar el update desde dev**: no aplica — el launcher y el updater sólo corren desde `%LocalAppData%\Nooka\`, no desde Electron.
- **Ver los logs**: en dev se abren en `%LocalAppData%\Nooka\nooka_launcher.log` y `SquirrelSetup.log` **si ya lanzaste la app empaquetada** alguna vez.

---

## 4 · Rebuild de cada pieza por separado

### 4.0 · Recursos del PE (VERSIONINFO, manifiesto e icono)

Los tres crates Rust llevan al lado de `src/`:

| Archivo | Qué mete | Quién lo compila |
|---|---|---|
| `resources.rc` | `ICON` + `1 VERSIONINFO` (CompanyName/FileDescription/FileVersion/OriginalFilename/ProductName/LegalCopyright) | `build.rs` con windres |
| `app.manifest` | `requestedExecutionLevel asInvoker` (RT_MANIFEST, id 1, tipo 24) | igual, vía `1 24 "app.manifest"` |
| `icon.ico` | el icono que se ve en el Explorador | igual |

`build.rs` llama a `C:\Users\ediss\Downloads\mingw64\bin\windres.exe`, enlaza el `.o` resultante y **vuelve a forzar `-Wl,--subsystem,windows`** (con recursos embebidos el linker GNU cambia el subsistema a consola si no se lo recuerdas). Verificado medido: los tres binarios salen con `Subsystem = 2` (GUI).

**Al bump de versión hay que tocar los tres `resources.rc` a mano.** No se autofijan: el launcher y el updater no conocen su versión (escanean `app-*/`), así que no hay ninguna constante de build de la que sacarlos. Un `FILEVERSION` viejo en un binario nuevo es justo lo que un motor de reputación lee como "dropper genérico".

Comprobación sin arrancar nada:

```bash
node scripts/pe-resources.mjs <ruta>.exe        # debe listar ICON (3), GROUP_ICON (14), VERSION (16), MANIFEST (24)
powershell -NoProfile -Command "(Get-Item '<ruta>.exe').VersionInfo | Format-List *"
```

### 4.1 · Launcher Rust

```bash
cd C:/Users/ediss/Documents/APP/Nooka/nooka-launcher
cargo build --release
# salida: target/release/nooka_launcher.exe (~419 KB)
```

Se puede copiar a la instalación sin más para probar:
```bash
cp target/release/nooka_launcher.exe "C:/Users/ediss/AppData/Local/Nooka/Nooka.exe"
```

El launcher no tiene versión embebida: obtiene la actual escaneando los directorios `app-*/` de `%LocalAppData%\Nooka\` y tomando el de semver mayor. No lee `RELEASES`.

### 4.2 · Updater Rust

```bash
cd C:/Users/ediss/Documents/APP/Nooka/nooka-updater
cargo build --release
# salida: target/release/nooka_updater.exe (~452 KB)
```
```bash
cp target/release/nooka_updater.exe "C:/Users/ediss/AppData/Local/Nooka/Update.exe"
```

El updater tampoco tiene versión embebida. Escanea los directorios `app-*/` en `%LocalAppData%\Nooka\` y toma el mayor como "versión instalada". El `RELEASES` de la raíz sólo se usa como manifiesto de Squirrel, para el setup y para que otras herramientas de Squirrel puedan leerlo; nuestro launcher y nuestro updater **no** lo consultan.

### 4.3 · Setup Rust

OJO: el setup **embebe** el `.nupkg` completo + el launcher + el updater con `include_bytes!`. Cualquier rebuild del setup requiere primero regenerar `nooka-setup/nooka-full.nupkg`, `nooka-setup/Update.exe` y `nooka-setup/Nooka.exe`, o el setup instalará las versiones viejas.

```bash
cd C:/Users/ediss/Documents/APP/Nooka/nooka-setup
cargo build --release
# salida: target/release/nooka_setup.exe (~108 MB)
```

Flujo para regenerar los embeds del setup:

```bash
# 1. Compilar launcher + updater (4.1, 4.2)
# 2. Copiar sus binarios a nooka-setup/ como Nooka.exe y Update.exe
cp ../nooka-launcher/target/release/nooka_launcher.exe ./Nooka.exe
cp ../nooka-updater/target/release/nooka_updater.exe   ./Update.exe
# 3. Reconstruir nooka-full.nupkg (ver §6.4)
# 4. Recompilar setup
cargo build --release
```

### 4.4 · `app.asar` del Electron

El asar tiene que tener **exactamente** esta estructura:

```
app.asar
├── package.json           ← { "name": "nooka-desktop", "version": "X.Y.Z", "main": "app/main.js" }
└── app/
    ├── main.js
    ├── preload.js
    ├── splash.html
    └── assets/
        ├── icon.ico
        └── icon-256.png
```

Si `main.js` va en la raíz del asar en vez de dentro de `app/`, Electron **muere silencioso** (sin error visible, sin ventana). Pasar `asar pack app` sin `package.json` al lado da ese mismo error. La recipe correcta:

```bash
mkdir -p /tmp/asar-stage/app
cp desktop/main.js desktop/preload.js desktop/splash.html /tmp/asar-stage/app/
cp -r desktop/assets /tmp/asar-stage/app/
cat > /tmp/asar-stage/package.json <<'EOF'
{
  "name": "nooka-desktop",
  "version": "1.0.4",
  "main": "app/main.js"
}
EOF
cd desktop
npx asar pack /tmp/asar-stage app-X.Y.Z/resources/app.asar
npx asar list app-X.Y.Z/resources/app.asar | head   # verifica que sale \app\main.js y \package.json
```

### 4.5 · `app-X.Y.Z/` completo (runtime Electron + asar)

OJO con la base: **el `node_modules/electron/dist` de `desktop/` no es el runtime que se ha publicado nunca.** Hoy el dist vale Electron 42.11.18 (`electron.exe` de 235 MB), mientras que los `Nooka.exe` empaquetados de 1.0.1→1.0.3 valen **Electron 28.3.3** (Chromium 120, 176 MB, byte-identico entre sí). Cambiar de base en un release "de limpieza" dispara el payload de 107 MB a ~150 MB sin pedirlo; si algún día se hace, es un bump de Electron deliberado, con su propia prueba.

La base real es el árbol ya empaquetado:

```bash
VER=1.0.4
cp -r nooka-dist/app-1.0.1 nooka-dist/app-$VER     # Nooka.exe 28.3.3 + dlls + locales
rm -f nooka-dist/app-$VER/debug.log                # basura de una sesión, no va al release
cp nooka-dist/app.asar.new nooka-dist/app-$VER/resources/app.asar
```

`nooka-dist/app-1.0.1/version` dice `1.0.1`: es el archivo `version` de Electron, que alguien sobrescribió una vez y desde entonces se arrastra. Nadie lo lee en runtime (`app.getVersion()` sale del `package.json` del asar), así que se deja como está.

El `Nooka.exe` de Electron ya trae embebido `app/` (el propio runtime). Renombrarlo es suficiente; no hay que reempaquetar el PE.

---

## 5 · Los dos layouts de `.nupkg` (fuente de confusiones)

Hay **dos** tipos de `.nupkg` conviviendo en este repo, y son incompatibles entre sí:

| Archivo | Layout | Quién lo lee | Qué hace |
|---|---|---|---|
| `nooka-setup/nooka-full.nupkg` | NuGet clásico: `lib/net45/*` | El **setup** (`nooka-setup/src/main.rs`) | Lo descomprime a `.staging`, luego **mueve** `lib/net45/` a `app-X.Y.Z/`. Convierte el layout. |
| `nooka.X.Y.Z.nupkg` (asset en GitHub) | Squirrel: `app-X.Y.Z/*` | El **updater** (`nooka-updater/src/main.rs`) | Lo descomprime y **espera** `app-X.Y.Z/` en la raíz del ZIP. Si sólo hay `lib/net45/`, falla con "No app directory found in staging". |

Por eso el script viejo `package-release.ps1` (que generaba `lib/net45/`) **no sirve** para el updater. El `.nupkg` de GitHub Release tiene que tener `app-X.Y.Z/` en la raíz. Ver §6.

OJO II: PowerShell `Expand-Archive` **rechaza** archivos con extensión `.nupkg`. El updater los descarga a disco siempre como `update.zip` y luego los descomprime.

---

## 6 · Build completo de una release nueva (v1.0.4, v1.0.5, ...)

Todo junto. Supongamos que vas a publicar la versión `1.0.4`.

### 6.1 · Bump de versión en todas las fuentes

- `nooka-dist/build-nupkg/nooka.nuspec` — `<version>1.0.4</version>` (de ahí saca el setup el nombre `app-X.Y.Z/` que crea). OJO: el `nooka-dist/nooka.nuspec` que sigue en el árbol está viejo (`1.0.1`) y **no lo lee nadie**; el que viaja dentro de los dos `.nupkg` es el de `build-nupkg`, comprobado byte a byte contra el nuspec embebido en `nooka.1.0.4.nupkg` y `nooka-full.nupkg`.
- El `package.json` del asar (se escribe en §6.3) — `"version": "1.0.4"`; es lo que ve `app.getVersion()`.
- `nooka-launcher/resources.rc`, `nooka-updater/resources.rc`, `nooka-setup/resources.rc` — `FILEVERSION`/`PRODUCTVERSION` y sus strings `FileVersion`/`ProductVersion`/`LegalCopyright`. Son **a mano**, ver §4.0.
- `desktop/package.json` — sólo afecta a dev, pero conviene mantenerlo alineado.

### 6.2 · Recompilar Rust (launcher + updater)

```bash
cd nooka-launcher && cargo build --release && cd ..
cd nooka-updater  && cargo build --release && cd ..
```

Con `CARGO_HOME=C:/Rust/cargo` y `RUSTUP_HOME=C:/Rust/rustup` exportados. Salida: `nooka_launcher.exe` (~420 KB) y `nooka_updater.exe` (~458 KB).

### 6.3 · Reconstruir el `app-1.0.4/` completo

```bash
VER=1.0.4
DEST="C:/Users/ediss/Documents/APP/Nooka/nooka-dist/app-$VER"

# a) Staging base: el árbol ya empaquetado (Electron 28.3.3), NO node_modules/electron/dist
rm -rf "$DEST"
cp -r nooka-dist/app-1.0.1 "$DEST"
rm -f "$DEST/debug.log"

# b) Empaquetar el asar con la estructura correcta (ver §4.4)
STAGE=/tmp/asar-v$VER
rm -rf $STAGE; mkdir -p $STAGE/app
cp desktop/main.js desktop/preload.js desktop/splash.html $STAGE/app/
cp -r desktop/assets $STAGE/app/
cat > $STAGE/package.json <<EOF
{"name":"nooka-desktop","version":"$VER","main":"app/main.js"}
EOF
(cd desktop && npx asar pack $STAGE "$DEST/resources/app.asar")

# c) Verificar estructura
(cd desktop && npx asar list "$DEST/resources/app.asar") | head
#   → debe salir \app\main.js, \app\preload.js, \package.json
```

OJO con `npx asar extract-file`: escribe el archivo en **el CWD actual**, no donde le indiques. Correrlo desde `desktop/` para "mirar" el `main.js` que hay dentro de un asar **te pisa `desktop/main.js`**. Para inspeccionar, haz `cd /tmp/algo` primero.

### 6.4 · Generar el `.nupkg` del setup (`lib/net45` + Update.exe + Nooka.exe)

```bash
BUILD="./nooka-dist/build-nupkg"
rm -rf "$BUILD/lib/net45"
cp -r "nooka-dist/app-1.0.4" "$BUILD/lib/net45"
cp nooka-updater/target/release/nooka_updater.exe  "$BUILD/Update.exe"
cp nooka-launcher/target/release/nooka_launcher.exe "$BUILD/Nooka.exe"

# ZIP siempre a un sitio FUERA del árbol que se comprime (si se emite dentro, CreateFromDirectory
# puede listarse a sí mismo) y con Add-Type, que PowerShell 5.1 no trae el tipo cargado.
powershell -NoProfile -Command "Add-Type -AssemblyName System.IO.Compression.FileSystem; [System.IO.Compression.ZipFile]::CreateFromDirectory('C:\Users\ediss\Documents\APP\Nooka\nooka-dist\build-nupkg', 'C:\Users\ediss\AppData\Local\Temp\nooka-full.zip')"
mv /c/Users/ediss/AppData/Local/Temp/nooka-full.zip nooka-setup/nooka-full.nupkg
```

`lib\net45\` (10 caracteres) y `app-1.0.4\` (10) miden igual, así que los dos `.nupkg` pueden quedar en **exactamente el mismo tamaño** y no por eso son el mismo archivo: se comprueban por SHA1, nunca por bytes.

OJO: PowerShell 5.1 escribe las entradas con **barra invertida** (`lib\net45\Nooka.exe`). No es un problema: `tar.exe` de System32 (bsdtar) trata `\` como separador en Windows y monta los directorios reales, medido con el asset de verdad. Quien sí se rompía con eso era `Expand-Archive`, que además rechazaba la extensión `.nupkg` — ésa es la razón por la que el pipeline ya no usa PowerShell.

### 6.5 · Recompilar el setup (embebe el nupkg)

```bash
cd nooka-setup
cp ../nooka-launcher/target/release/nooka_launcher.exe ./Nooka.exe
cp ../nooka-updater/target/release/nooka_updater.exe   ./Update.exe
cargo build --release
# → target/release/nooka_setup.exe (~108 MB)
```

### 6.6 · Generar el `.nupkg` para el **updater** (Squirrel layout: `app-1.0.4/` en raíz)

```bash
STAGE="C:/Users/ediss/Documents/APP/Nooka/nooka-dist/build-squirrel"
rm -rf $STAGE; mkdir -p $STAGE
cp nooka-dist/build-nupkg/nooka.nuspec $STAGE/
cp -r nooka-dist/app-1.0.4 $STAGE/app-1.0.4
# El .nupkg del release necesita también Update.exe y Nooka.exe en la raíz
cp nooka-updater/target/release/nooka_updater.exe  $STAGE/Update.exe
cp nooka-launcher/target/release/nooka_launcher.exe $STAGE/Nooka.exe

powershell -NoProfile -Command "Add-Type -AssemblyName System.IO.Compression.FileSystem; [System.IO.Compression.ZipFile]::CreateFromDirectory('C:\Users\ediss\Documents\APP\Nooka\nooka-dist\build-squirrel', 'C:\Users\ediss\Documents\APP\Nooka\nooka-dist\nooka.1.0.4.nupkg')"
rm -rf $STAGE
```

Los dos exe de la raíz **ya no son decorativos**: `install_update` los copia encima de los instalados (`refresh_root_binaries` en `nooka-updater/src/main.rs`). Antes de eso, un update traía la app nueva pero dejaba el launcher y el updater viejos para siempre, así que ningún arreglo de esos binarios llegaba a quien no reinstalara desde cero. Por el mismo motivo, tras un update `Update.exe`/`Nooka.exe` quedan como `*.old` mientras vive el proceso que se apartó a sí mismo, y los barre **la siguiente arrancada** (el barrido está al principio de `main()`, no en `cleanup_temp_files`, que corre después de instalar cuando la ranura sigue bloqueada).

Si el ZIP se queda sin eliminar por `Device or resource busy`, es Windows (Defensor/indexador) con el `app.asar` recién escrito: reintenta `rm -rf` unos minutos después, no insistas en un bucle.

### 6.7 · Calcular el SHA1 y escribir `RELEASES`

```bash
cd nooka-dist
SHA=$(certutil -hashfile nooka.1.0.4.nupkg SHA1 | sed -n '2p' | tr -d ' \r')
SIZE=$(stat -c %s nooka.1.0.4.nupkg)
echo "$SHA nooka.1.0.4.nupkg $SIZE" > RELEASES
cat RELEASES
```

Formato exacto: `<sha1>  <nombre-nupkg>  <bytes>` en una línea. Lo dejamos porque lo esperan las herramientas clásicas de Squirrel y porque `nooka-dist/RELEASES` forma parte del release visible en GitHub. **Nuestro launcher y nuestro updater no lo leen**: para la versión instalada escanean `app-*/`, y para la versión nueva llaman a la API `/releases/latest` de GitHub.

### 6.8 · Publicar el release en GitHub

**Pre-vuelo** (cada una de estas cosas ya mordió):

```bash
# 1. Los cinco assets existen y son del día
ls -la nooka-dist/nooka.1.0.4.nupkg nooka-dist/RELEASES nooka-dist/Update.exe \
       nooka-launcher/target/release/nooka_launcher.exe nooka-setup/target/release/nooka_setup.exe
# 2. RELEASES bate con el asset
certutil -hashfile nooka-dist/nooka.1.0.4.nupkg SHA1 | sed -n 2p   # == primera columna de RELEASES
# 3. Ningún binario nuestro menciona ya PowerShell / Expand-Archive / WScript
grep -ac powershell nooka-dist/Update.exe   # → 0
# 4. Identidad del PE en los tres
node scripts/pe-resources.mjs <cada exe>     # ICON + VERSION + MANIFEST
# 5. El asar empaquetado es el de hoy y dice 1.0.4
(cd desktop && npx asar list ../nooka-dist/app-1.0.4/resources/app.asar) | head
mkdir -p /tmp/asarcheck && cd /tmp/asarcheck && \
  /c/Users/ediss/Documents/APP/Nooka/desktop/node_modules/.bin/asar.cmd extract-file \
  /c/Users/ediss/Documents/APP/Nooka/nooka-dist/app-1.0.4/resources/app.asar package.json && cat package.json
#   (extract-file escribe en el CWD: por eso el cd /tmp antes, si no te pisa desktop/package.json)
```

`nooka-dist/Update.exe` y `nooka-dist/Nooka.exe` son copias visibles en el repo de los binarios de `target/release/`; hay que refrescarlas en cada release o el asset subido será el viejo.

```bash
gh release create v1.0.4 \
  --repo uzizsisas777/nooka \
  --title "Nooka v1.0.4" \
  --notes "Qué cambió en esta versión." \
  nooka-dist/nooka.1.0.4.nupkg \
  nooka-dist/RELEASES \
  nooka-dist/Update.exe \
  nooka-launcher/target/release/nooka_launcher.exe#Nooka.exe \
  nooka-setup/target/release/nooka_setup.exe#NookaSetup.exe
```

OJO con el `#Nooka.exe`: **NO renombra el asset**, le pone *display label*. Lo dice `gh release create --help` (2.102.0) con esas palabras: *"To define a display label for an asset, append text starting with `#`"*. Medido el 2026-11-11 al publicar v1.0.4: `nooka_setup.exe#NookaSetup.exe` quedó colgado con `name = nooka_setup.exe`, así que la url de descarga era `…/download/v1.0.4/nooka_setup.exe`. Los `.nupkg` y los que ya salen bien nombrados de `nooka-dist/` tapan el fallo, pero el setup y el launcher **no** (Rust los escribe en minúsculas). Lo que sí cambia el nombre es un `PATCH` sobre el asset:

```bash
gh api -X PATCH repos/uzizsisas777/nooka/releases/assets/<ID> -f name="NookaSetup.exe"
```

El `<ID>` se saca de `gh api repos/uzizsisas777/nooka/releases/tags/v1.0.4 --jq '.assets[]|"\(.id)\t\(.name)"'`. Importa de verdad y no es cosmética: el `OriginalFilename` que metimos en el VERSIONINFO del setup es `NookaSetup.exe`, y un archivo en disco que no coincide con su `OriginalFilename` es justo una de las señas que miran los motores. Comprobado tras el `PATCH`: `.name`, `.digest` y `browser_download_url` coherentes.

El release tiene que exponer estos **5 assets exactos** (verificado con `gh release view v1.0.3`):

```
nooka.1.0.4.nupkg        ← payload que descarga el updater
RELEASES                 ← manifiesto
Nooka.exe                ← launcher standalone (por si alguien lo quiere a mano)
Update.exe               ← updater standalone
NookaSetup.exe           ← instalador para usuarios nuevos
```

**Si el usuario abre una versión vieja y el updater busca `.nupkg`**, `find(|a| a.name.ends_with(".nupkg"))` lo toma. Sin ese asset, el update no arranca.

### 6.9 · Probar antes de publicar

Subir un draft primero:
```bash
gh release create v1.0.4 --draft ...  # como arriba pero con --draft
```
Abrir el draft en el navegador, copiar la URL del asset `.nupkg`, y correr el updater a mano en otra máquina con la app vieja:
```bash
"AppData/Local/Nooka/Update.exe" --update
```
Si descarga, instala, y la app arranca la nueva sin terminal de por medio, se publica:
```bash
gh release edit v1.0.4 --draft=false
```

---

## 7 · Update flow en runtime (qué pasa cuando haces doble clic)

```
usuario → doble clic Nooka.lnk
  │
  ├─ Windows → Start-Process "%LocalAppData%\Nooka\Update.exe --processStart Nooka.exe"
  │       ↑ subsistema=2 (Rust GNU), Windows regala una consola
  │       └ Update.exe main() → GetConsoleProcessList ≤ 1 → ShowWindow(SW_HIDE)
  │
  ├─ Update.exe parsea --processStart
  │       └ launch_app()
  │           ├ existe %LocalAppData%\Nooka\Nooka.exe (launcher)?
  │           │   → Command::new("Nooka.exe").creation_flags(CREATE_NO_WINDOW).spawn()
  │           └ no existe?
  │               → busca app-<latest>/Nooka.exe (Electron) y lanza directo
  │
  ├─ Nooka.exe (launcher) main()
  │       ├ Hide console (mismo truco)
  │       ├ create_splash_window() → ventana 420×320 con icon.ico, 1500 ms
  │       ├ run_message_loop() bloquea hasta WM_TIMER
  │       ├ DestroyWindow(splash)
  │       │
  │       ├ get_current_version() → lee RELEASES / app-* más alto
  │       ├ check_for_update()
  │       │   → powershell -Command "Invoke-WebRequest 'https://api.github.com/repos/uzizsisas777/nooka/releases/latest'"
  │       │     con CREATE_NO_WINDOW (nunca se ve)
  │       │   → serde_json → GitHubRelease { tag_name, assets[] }
  │       │
  │       ├ latest > current?
  │       │   └ SÍ: launch_updater()
  │       │       → Update.exe --update con CREATE_NO_WINDOW, .status() (espera)
  │       │         └ Update.exe (updater) main():
  │       │             Hide console → run_update_with_ui() → ventana de progreso nativa:
  │       │             ├ check_for_update() → misma API de GitHub
  │       │             ├ download_update() → powershell Invoke-WebRequest
  │       │             │     browser_download_url del asset .nupkg
  │       │             │     se guarda como .update-temp/update.zip (Expand-Archive exige .zip)
  │       │             ├ install_update()
  │       │             │     Expand-Archive update.zip → .staging/
  │       │             │     busca app-*/ en .staging/ (Squirrel layout raíz)
  │       │             │     mueve a %LocalAppData%\Nooka\app-1.0.4\
  │       │             │     purga: mantiene 2 versiones máximas
  │       │             └ cierra ventana progreso, exit 0
  │       │   └ NO: logger.info("No update available")
  │       │
  │       ├ find_latest_version() → escanea app-*/ y devuelve el mayor
  │       ├ launch_process(latest_path/Nooka.exe, args, wait=false)
  │       │     CREATE_NO_WINDOW (Electron no hereda consola)
  │       └ exit 0
  │
  ├─ app-1.0.4/Nooka.exe (Electron 42.11.18) main()
  │       ├ app.setAppUserModelId('com.nooka.desktop')
  │       ├ requestSingleInstanceLock() → si falla, app.quit() (Discord-style)
  │       ├ on('second-instance') → trae la ventana existente al frente
  │       ├ createSplash() → BrowserWindow 420×320 con splash.html
  │       ├ createMainWindow()
  │       │   ├ url = isDev ? 'http://localhost:5173' : 'https://nooka-web.vercel.app'
  │       │   ├ NOOKA_SERVER = 'http://127.0.0.1:3001'
  │       │   ├ loadURL(url)
  │       │   ├ did-finish-load → cierra splash, mainWindow.show()
  │       │   └ en dev: openDevTools detached
  │       └ IPC handlers: window:minimize/maximize/close, getVersion, getServerUrl
```

**Claves**:
- El launcher **no hace download ni install**. Delega todo al updater.
- El updater **no abre la app**. Sólo instala y muere. El launcher relanza.
- La ventana de progreso del updater es **Win32 nativa** (no Electron), por eso es tan liviana y arranca antes que nada de la app.
- Single instance lock es de **Electron**, no del launcher. Si haces doble clic dos veces, la segunda ejecución de `app-X.Y.Z/Nooka.exe` detecta la existente y la trae al frente.

---

## 8 · Qué hace cada flag / comando del updater

`Update.exe` (el mismo `.exe`) responde a **seis** comandos, parseados en `nooka-updater/src/main.rs::main`:

```
Update.exe --checkForUpdate       → consulta API, log, sin cambiar nada
Update.exe --download             → baja el .nupkg a .update-temp/, no instala
Update.exe --update               → descarga + instala + purga viejas + UI progreso
Update.exe --createShortcut       → (re)crea Desktop.lnk y StartMenu.lnk
Update.exe --removeShortcut       → borra los accesos directos
Update.exe --processStart Nooka.exe   → lo usa el atajo: lanza la app
```

Sin argumento: imprime usage y sale 0.

El **launcher** (`Nooka.exe`) no tiene un `--checkForUpdate`; siempre hace check al arrancar y lanza el updater si toca. Los flags que sí reconoce son los de Squirrel:

```
Nooka.exe                                    → splash + update check + launch
Nooka.exe --processStart app.exe             → usa ese exe en vez de Nooka.exe
Nooka.exe --processStartAndWait app.exe      → espera a que cierre
Nooka.exe --process-start-args "..."         → args extra al exe destino
Nooka.exe --no-ui                            → sin splash (para test)
```

---

## 9 · Trampas conocidas (todas mordieron alguna vez)

1. **`app.asar` sin `package.json` en la raíz** → Electron se cierra sin ningún mensaje de error. Siempre verificar con `npx asar list` que `\package.json` y `\app\main.js` estén ahí.
2. **`main.js` en la raíz del asar** (sin subcarpeta `app/`) → mismo fallo silencioso. Hay que pasarle al `asar pack` un staging con `app/main.js`, no `main.js` pelado.
3. **`.nupkg` con `lib/net45/`** → el updater tira "No app directory found in staging". El setup sí espera ese layout, pero el updater NO.
4. **PowerShell en el pipeline** → desde v1.0.4 ya no queda ninguno: `Expand-Archive` se cambió por `tar.exe` (bsdtar) de System32, `curl.exe` de System32 sustituye a `Invoke-RestMethod`/`WebClient`, y el atajo `.lnk` se escribe por COM (`IShellLinkW` + `IPersistFile`) en vez de `WScript.Shell`. **Motivo:** un intérprete de scripts que baja un archivo, lo descomprime y suelta exe + `.lnk` en AppData es la firma conductual de un dropper, y dos motores (Bkav y McAfee) nos marcaban `NookaSetup.exe` por ella. `WScript`/`Shell.Application`/`Expand-Archive` no volverán: cualquier "mejora" que reintroduzca `powershell.exe` desde un binario nuestro reactiva el veredicto.
5. **Consola visible al abrir el atajo** → con toolchain GNU, `#![windows_subsystem = "windows"]` no bastaba para cambiar el byte `Subsystem` del PE. Hoy los tres crates enlazan recursos y `build.rs` vuelve a pasar `-Wl,--subsystem,windows`, así que el PE ya declara **Subsystem=2 (GUI)** — medido en los binarios de v1.0.4. El `ShowWindow(GetConsoleWindow(), SW_HIDE)` gated con `GetConsoleProcessList ≤ 1` se queda igual: cuesta cero y sigue salvando el caso de que alguien ejecute el exe desde una consola con un linker distinto.
6. **Git identity** → los commits tienen que ser con `uzizsisas777` o Vercel/Railway bloquean el deploy.
7. **El updater SÍ se autoactualiza desde v1.0.4** (`refresh_root_binaries`). Antes, al instalar una `app-X.Y.Z/` nueva los `Nooka.exe`/`Update.exe` de la raíz se quedaban viejos para siempre; ahora se apartan a `*.old`, se copian los del paquete y el `.old` lo borra la siguiente arrancada. OJO: si un `.nupkg` no trae esos dos exe en la raíz, el updater **deja los que hay** y lo anota en el log (no borra nada).
8. **El server del paquete ya no está embebido fijo.** `desktop/main.js` lee `process.env.NOOKA_SERVER_URL` y, si no se fija, en paquete vale `null`: la app carga la web desplegada y ésa trae su propio origen. En dev sigue `http://127.0.0.1:3001`.
9. **El updater busca el asset `.nupkg` en `/latest`**. Si el release no lo tiene (por ejemplo, sólo subiste `NookaSetup.exe`), `install_update` falla con "No .nupkg or .zip asset found in release".
10. **Draft releases** no aparecen en `/latest`. Tienes que publicar (`gh release edit --draft=false`) para que el updater los vea.
11. **`winapi::ctypes::c_void`, no `std::os::raw::c_void`** → los `CoCreateInstance`/`QueryInterface` de winapi 0.3 piden `*mut *mut c_void` con el `c_void` de *winapi*; con el de `std` no enlazan (tipos distintos). Y `CLSID_ShellLink` **no está en winapi 0.3**: va escrito a mano `{00021401-0000-0000-C000-000000000046}`.
12. **`SetIconLocation` recibe la ruta PELADA y el índice por su propio argumento.** La sintaxis `"ruta.exe,0"` es de `WScript.Shell`, no de `IShellLinkW`; si le metes `,0` dentro de la ruta el atajo guarda un archivo inexistente y el icono sale roto (éste fue el bug que sólo aparecía al probar, no al compilar).
13. **`resources.rc` sólo ASCII** y con `1 VERSIONINFO … END` + `1 24 "app.manifest"` sin ningún `#include`: windres compila con la codepage del sistema y un carácter raro tumba el build con un error opaco.

---

## 10 · Prueba local del update flow sin GitHub

Para probar el update sin subir un release nuevo a la repo pública, apunta al tuyo propio:

```bash
# 1. Fork en uzizsisas777/nooka-test
# 2. Cambia la URL en nooka-launcher/src/main.rs:33 Y nooka-updater/src/main.rs:36
#    RELEASES_URL = "https://api.github.com/repos/uzizsisas777/nooka-test/releases/latest"
# 3. Recompilar launcher + updater
# 4. Subir release v9.9.9 al repo de prueba con los 5 assets
# 5. Abrir Nooka.exe local → detecta v9.9.9 → actualiza
```

O sin recompilar nada, corriendo el updater a mano con una URL local:

```bash
# Simula que GitHub ya devolvió el release: instala un ZIP a mano
cd %LocalAppData%\Nooka
mkdir -p .update-temp
cp /ruta/al/nooka-candidate.nupkg .update-temp/update.zip
Update.exe --update   # leerá el zip staging, no hará download si ya está
```

Ojo: la implementación actual hace download SIEMPRE en `run_update`, no comprueba si `.update-temp/update.zip` ya existe. Si quieres probar eso, edita `nooka-updater/src/main.rs::download_update` para saltar si el archivo ya está en disco.

---

## 11 · Dónde mirar cuando algo falla

| Síntoma | Log / comando |
|---|---|
| App no arranca | `%LocalAppData%\Nooka\nooka_launcher.log` |
| Update dice "No .nupkg asset" | `%LocalAppData%\Nooka\SquirrelSetup.log` → revisa assets del release en la web |
| Update descarga pero no instala | `SquirrelSetup.log` línea "Extracting update" — suele ser `lib/net45/` vs `app-*/` |
| Ventana Electron en negro | `app-X.Y.Z/resources/app.asar` mal formado → `npx asar list` |
| Consola flash al abrir | Verificar que `ShowWindow(GetConsoleWindow(), SW_HIDE)` está en el main() de launcher + updater |
| Dos instancias abren | `requestSingleInstanceLock()` en `main.js` no está corriendo — ¿editaste el asar? |
| Usuario nuevo no puede instalar | Bajar `NookaSetup.exe` del release, ejecutarlo con doble clic, ver `SquirrelSetup.log` |

---

## 12 · Comandos que vas a usar siempre

```bash
# ---- Dev ----
cd C:/Users/ediss/Documents/APP/Nooka/desktop
npx electron .                        # arranca la app Electron en modo dev

# ---- Rebuild binarios Rust ----
cd C:/Users/ediss/Documents/APP/Nooka
(cd nooka-launcher && cargo build --release)
(cd nooka-updater  && cargo build --release)

# ---- Probar launcher/updater sin romper la instalación real ----
./nooka-launcher/target/release/nooka_launcher.exe --no-ui --processStart Nooka.exe
./nooka-updater/target/release/nooka_updater.exe   --checkForUpdate
./nooka-updater/target/release/nooka_updater.exe   --update        # descarga + instala

# ---- Deploy a la instalación real para testear el update flow ----
cp nooka-launcher/target/release/nooka_launcher.exe "$LOCALAPPDATA/Nooka/Nooka.exe"
cp nooka-updater/target/release/nooka_updater.exe   "$LOCALAPPDATA/Nooka/Update.exe"

# ---- Ver releases en GitHub ----
gh release list -R uzizsisas777/nooka
gh release view v1.0.4 -R uzizsisas777/nooka
gh release download v1.0.4 -R uzizsisas777/nooka -p "*.nupkg" -D /tmp/test

# ---- Ver releases en la instalación local ----
type "$LOCALAPPDATA/Nooka/RELEASES"
dir "$LOCALAPPDATA/Nooka" | findstr "app-"

# ---- Borrar instalación y empezar limpio ----
taskkill /F /IM Nooka.exe 2>nul
taskkill /F /IM Update.exe 2>nul
rmdir /S /Q "%LocalAppData%\Nooka"
```

---

## 13 · Referencias cruzadas

- Código de las cuatro piezas: `nooka-launcher/src/main.rs`, `nooka-updater/src/main.rs`, `nooka-setup/src/main.rs`, `desktop/main.js`.
- Scripts viejos en la raíz del repo (`package-release.ps1`, `create-github-release.ps1`) quedaron en desuso; son de antes de que el updater esperara Squirrel layout.
- Documentación de arquitectura general del proyecto: `AGENTS.md` en la raíz.
- Evidencia del diseño de Discord que inspiró este pipeline: `docs/discord-rea-debug.md` y la memoria `project-discord-launcher-recreado.md`.

---

## 14 · Falsos positivos de antivirus

Estado medido del `NookaSetup.exe` de **v1.0.3** (SHA256 `594976e46979d86169cf1c5ca301fc0ef517385e9885e51047d5b76931e265b84`, 108.974.080 B) en VirusTotal: **2 de 69** motores.

| Motor | Etiqueta | De dónde sale |
|---|---|---|
| McAfee/Trellix | `TIIS94976E46979` | el prefijo `94976E46979` **es un trozo del SHA256 del archivo**: veredicto por hash, no por análisis |
| Bkav | `W32.Malware.8624F32C` | igual: `8624F32C` es fragmento del hash |

No hay malware: el setup embebe un ZIP de 108 MB, lo descomprime en `%LocalAppData%\Nooka` y crea un atajo. Lo que **sí** explicaba las banderas (todas en las etiquetas de VT: `peexe, 64bits, self-delete, macro-powershell, obfuscated, crypto, calls-wmi, checks-network-adapters, qr-code`):

1. **Comportamiento de dropper**: `powershell.exe` → descargar → descomprimir → escribir exe → crear `.lnk`. **Resuelto en v1.0.4** con `curl.exe`/`tar.exe` de System32 y `IShellLinkW` por COM (ver trampa 4).
2. **Cero identidad**: sin VERSIONINFO, sin manifiesto, sin icono, sin nombre de archivo original. **Resuelto en v1.0.4** (§4.0).
3. **Sin firma Authenticode y sin reputación de versión previa**. **No resuelto**: no hay ruta viable a coste cero. Azure Trusted Signing pide tarjeta y 9,99 $/mes; SignPath exige licencia OSI y cero código propio. Es la única cura estructural que queda, y depende de pagar, no de programar.

Mientras tanto, lo que baja los veredictos de hash-genéricos es **que el hash cambie** (v1.0.4 lo hace: `NookaSetup.exe` SHA256 `8b129e3d2e7e6b1865f209831ba72ecc5d585969d6e27277ff1e62a5f668e182`) **y reportar el falso positivo** con el archivo nuevo. Los formularios están en §14.1.

### 14.1 · Dónde se reporta

Contactos verificados el 2026-11-11 (uno no lo estaba y fuera):

- **Trellix (antes McAfee)**: artículo de su base de conocimiento *How to submit samples to Trellix Advanced Research Center* — <https://support.trellix.com/s/article/KB68030> — y el correo del laboratorio <datasubmission@trellix.com>. La otra vía es un ticket en el portal de soporte de Trellix marcándolo como *false-positive submission*. **NO existe** la página `trellix.com/security-services/false-positive-submission/` que estaba escrita aquí antes.
- **Bkav**: no tiene formulario web en inglés. Los correos del laboratorio son <fpreport@bkav.com> y <bkav@bkav.com>. Es un motor con peso mínimo en la detección real; se reporta por cortesía. **NO existe** `antivirus.com.vn` como contacto de FP.
- **Microsoft Defender** (no nos marcó, pero es el que más importa para el resto): <https://www.microsoft.com/en-us/wdsi/filesubmission> → la categoría *False positive*. Un filing de Microsoft suele propagar reputación al resto de motores que consumen su telemetría. Ojo: el formulario **pide el archivo**, no sólo el hash —y el setup pesa 104 MB, así que hay que tenerlo a mano—.
- **VirusTotal**: NO acepta reportes de FP él mismo, sólo agrega los de los fabricantes (lo dice su propia documentación). Los tres pasos reales: el botón **Reanalyse** del informe; después **reportarse contra cada fabricante por su portal** (la lista está en su *contributors page*); y <https://www.virustotal.com/gui/contact-us/false-positive> es el **False Positive form**, que sirve para que limpien un resultado en caché cuando ya tienes la confirmación del fabricante, no para discutir el veredicto. La nota que decía «cuesta 1 crédito» no está respaldada por su documentación: donde hay créditos es en la API, no en este formulario.

Un informe dice: qué hace el programa, que es autoextraíble estilo Squirrel, dónde escribe, que no toca red más allá de `api.github.com` para `releases/latest`, y adjunta los hashes viejo y nuevo.

### 14.2 · Qué vigilar en el siguiente release

- Si VT vuelve a marcar `macro-powershell` o `obfuscated`, es que algo reintrodujo un intérprete de scripts → revisar los tres crates.
- Los `resources.rc` con versión vieja son un retroceso directo: un exe de 1.0.5 gritando `FileVersion 1.0.4` es exactamente la pinta de un reempaquetado.
- Cada release nuevo cambia los cinco hashes; los reportes de FP hay que rehacerlos por versión (un filing contra el hash de 1.0.4 no libera el de 1.0.5).
