#![windows_subsystem = "windows"]

use semver::Version;
use serde::Deserialize;
use std::env;
use std::fs;
use std::io::Write;
use winapi::ctypes::c_void;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use widestring::U16CString;
use winapi::shared::guiddef::GUID;
use winapi::shared::minwindef::{DWORD, FALSE, LPARAM, LRESULT, WPARAM};
use winapi::shared::winerror::S_OK;
use winapi::um::combaseapi::{CoCreateInstance, CoInitializeEx, CoUninitialize};
use winapi::um::objidl::IPersistFile;
use winapi::um::shobjidl_core::IShellLinkW;
use winapi::Interface;
use winapi::shared::windef::{HDC, HWND, RECT};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winbase::CREATE_NO_WINDOW;
use winapi::um::wincon::{GetConsoleProcessList, GetConsoleWindow};
use winapi::um::wingdi::{
    CreateFontW, CreateSolidBrush, DeleteObject, SelectObject, SetBkMode, SetTextColor,
    RGB, TRANSPARENT,
};
use winapi::um::winuser::{
    BeginPaint, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, DispatchMessageW,
    DrawIconEx, DrawTextW, EndPaint, FillRect, GetClientRect, GetMessageW, GetSystemMetrics,
    InvalidateRect, LoadCursorW, LoadImageW, MessageBoxW, PostMessageW, PostQuitMessage,
    RegisterClassExW, SetTimer, ShowWindow, TranslateMessage, UpdateWindow, DT_LEFT,
    DT_SINGLELINE, DT_VCENTER, IDC_ARROW, IMAGE_ICON, LR_LOADFROMFILE, MB_ICONERROR, MB_OK,
    PAINTSTRUCT, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE, SW_SHOW, WM_APP, WM_CREATE, WM_DESTROY, WM_PAINT,
    WM_TIMER, WNDCLASSEXW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

const APP_PREFIX: &str = "app-";
const RELEASES_URL: &str = "https://api.github.com/repos/uzizsisas777/nooka/releases/latest";
const MAX_OLD_VERSIONS: usize = 2;
const MARQUEE: usize = usize::MAX;
const WINDOW_CLASS: &str = "NookaUpdaterProgress";
const WM_UPDATE_DONE: u32 = WM_APP + 1;

// IShellLinkW e IPersistFile traen su IID de winapi; el CLSID del objeto ShellLink no está en
// winapi 0.3 y vale {00021401-0000-0000-C000-000000000046}.
const CLSID_SHELL_LINK: GUID = GUID {
    Data1: 0x00021401,
    Data2: 0x0000,
    Data3: 0x0000,
    Data4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
};
const CLSCTX_INPROC_SERVER: DWORD = 0x1;
const COINIT_APARTMENTTHREADED: DWORD = 0x2;

#[derive(Deserialize, Debug)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize, Debug)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

struct UpdaterConfig {
    nooka_dir: PathBuf,
    command: String,
}

impl UpdaterConfig {
    fn from_args() -> Self {
        let args: Vec<String> = env::args().collect();
        let exe_path = env::current_exe().unwrap_or_default();
        let nooka_dir = exe_path.parent().unwrap_or(Path::new(".")).to_path_buf();

        let mut config = UpdaterConfig {
            nooka_dir,
            command: String::new(),
        };

        for arg in args.iter().skip(1) {
            match arg.as_str() {
                "--checkForUpdate" | "--download" | "--update" | "--createShortcut"
                | "--removeShortcut" | "--processStart" => {
                    config.command = arg.clone();
                }
                _ => {}
            }
        }

        config
    }
}

struct Logger {
    log_file: Option<fs::File>,
}

impl Logger {
    fn new(nooka_dir: &Path) -> Self {
        let log_path = nooka_dir.join("SquirrelSetup.log");
        let log_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .ok();

        Self { log_file }
    }

    fn log(&mut self, message: &str) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let line = format!("[{}] {}\n", timestamp, message);
        eprintln!("[Update.exe] {}", message);

        if let Some(ref mut file) = self.log_file {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }
}

// ---------- Progress UI ----------

struct UiState {
    percent: AtomicUsize,
    status: Mutex<String>,
    result: Mutex<Option<Result<(), String>>>,
}

static UI: OnceLock<Arc<UiState>> = OnceLock::new();

fn set_status(state: &UiState, text: &str) {
    if let Ok(mut status) = state.status.lock() {
        *status = text.to_string();
    }
}

fn show_error_box(message: &str) {
    let text = U16CString::from_str(message).unwrap_or_default();
    let title = U16CString::from_str("Nooka").unwrap_or_default();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

unsafe fn draw_text(hdc: HDC, text: &str, rect: &RECT, size: i32, weight: i32, color: u32) {
    let wide = match U16CString::from_str(text) {
        Ok(w) => w,
        Err(_) => return,
    };
    let font_name = U16CString::from_str("Segoe UI").unwrap();
    SetTextColor(hdc, color);
    let font = CreateFontW(
        size, 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 0, 0,
        font_name.as_ptr(),
    );
    let old = SelectObject(hdc, font as *mut _);
    DrawTextW(
        hdc,
        wide.as_ptr(),
        -1,
        rect as *const _ as *mut _,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER,
    );
    SelectObject(hdc, old);
    DeleteObject(font as *mut _);
}

unsafe fn paint_progress(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rect: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rect);

    let bg = CreateSolidBrush(RGB(32, 34, 37));
    FillRect(hdc, &rect, bg);
    DeleteObject(bg as *mut _);
    SetBkMode(hdc, TRANSPARENT as i32);

    // Icon next to the updater
    let exe = env::current_exe().unwrap_or_default();
    let icon_path = exe.parent().unwrap_or(Path::new(".")).join("icon.ico");
    if let Ok(wide) = U16CString::from_str(&icon_path.to_string_lossy()) {
        let hicon = LoadImageW(
            std::ptr::null_mut(),
            wide.as_ptr(),
            IMAGE_ICON,
            32,
            32,
            LR_LOADFROMFILE,
        );
        if !hicon.is_null() {
            DrawIconEx(hdc, 20, 22, hicon as *mut _, 32, 32, 0, std::ptr::null_mut(), 0x0003);
            DestroyIcon(hicon as *mut _);
        }
    }

    let title_rect = RECT { left: 64, top: 18, right: 400, bottom: 42 };
    draw_text(hdc, "Actualizando Nooka", &title_rect, 20, 700, RGB(255, 255, 255));

    let state = match UI.get() {
        Some(s) => s,
        None => {
            EndPaint(hwnd, &ps);
            return;
        }
    };

    let status = state.status.lock().map(|s| s.clone()).unwrap_or_default();
    let status_rect = RECT { left: 64, top: 44, right: 404, bottom: 66 };
    draw_text(hdc, &status, &status_rect, 14, 400, RGB(181, 186, 193));

    // Progress track
    let track = RECT { left: 20, top: 84, right: 400, bottom: 92 };
    let track_brush = CreateSolidBrush(RGB(64, 68, 75));
    FillRect(hdc, &track, track_brush);
    DeleteObject(track_brush as *mut _);

    let fill_brush = CreateSolidBrush(RGB(88, 101, 242));
    let pct = state.percent.load(Ordering::Relaxed);
    if pct == MARQUEE {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let x = 20 + ((millis / 6) % 460) as i32 - 80;
        let left = x.max(20);
        let right = (x + 80).min(400);
        if right > left {
            let fill = RECT { left, top: 84, right, bottom: 92 };
            FillRect(hdc, &fill, fill_brush);
        }
    } else {
        let width = (380 * pct.min(100) / 100) as i32;
        if width > 0 {
            let fill = RECT { left: 20, top: 84, right: 20 + width, bottom: 92 };
            FillRect(hdc, &fill, fill_brush);
        }
    }
    DeleteObject(fill_brush as *mut _);

    EndPaint(hwnd, &ps);
}

unsafe extern "system" fn progress_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            SetTimer(hwnd, 1, 80, None);
            0
        }
        WM_TIMER => {
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_PAINT => {
            paint_progress(hwnd);
            0
        }
        WM_UPDATE_DONE => {
            PostQuitMessage(0);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

fn create_progress_window() -> Option<HWND> {
    unsafe {
        let class_name = U16CString::from_str(WINDOW_CLASS).ok()?;
        let window_name = U16CString::from_str("Actualizando Nooka").ok()?;

        let mut wcex: WNDCLASSEXW = std::mem::zeroed();
        wcex.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wcex.lpfnWndProc = Some(progress_proc);
        wcex.hInstance = GetModuleHandleW(std::ptr::null());
        wcex.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
        wcex.lpszClassName = class_name.as_ptr();

        if RegisterClassExW(&wcex) == 0 {
            return None;
        }

        let width = 420;
        let height = 120;
        let x = (GetSystemMetrics(SM_CXSCREEN) - width) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - height) / 2;

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST,
            class_name.as_ptr(),
            window_name.as_ptr(),
            WS_POPUP | WS_VISIBLE,
            x,
            y,
            width,
            height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            wcex.hInstance,
            std::ptr::null_mut(),
        );

        if hwnd.is_null() {
            return None;
        }

        ShowWindow(hwnd, SW_SHOW);
        UpdateWindow(hwnd);
        Some(hwnd)
    }
}

fn run_update_with_ui(nooka_dir: &Path) -> Result<(), String> {
    let state = Arc::new(UiState {
        percent: AtomicUsize::new(MARQUEE),
        status: Mutex::new("Buscando actualizaciones...".to_string()),
        result: Mutex::new(None),
    });
    let _ = UI.set(state.clone());

    let hwnd = create_progress_window().ok_or("Could not create progress window")?;
    let hwnd_raw = hwnd as isize;
    let worker_state = state.clone();
    let dir = nooka_dir.to_path_buf();

    thread::spawn(move || {
        let mut logger = Logger::new(&dir);
        let result = run_update(&worker_state, &dir, &mut logger);
        if let Ok(mut slot) = worker_state.result.lock() {
            *slot = Some(result);
        }
        unsafe {
            PostMessageW(hwnd_raw as HWND, WM_UPDATE_DONE, 0, 0);
        }
    });

    unsafe {
        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        DestroyWindow(hwnd);
    }

    state
        .result
        .lock()
        .ok()
        .and_then(|mut slot| slot.take())
        .unwrap_or(Ok(()))
}

// ---------- Update logic ----------

fn releases_url() -> String {
    env::var("NOOKA_RELEASES_URL").unwrap_or_else(|_| RELEASES_URL.to_string())
}

fn get_current_version(nooka_dir: &Path) -> Option<Version> {
    get_all_versions(nooka_dir).last().map(|(v, _)| v.clone())
}

fn get_all_versions(nooka_dir: &Path) -> Vec<(Version, PathBuf)> {
    let entries = match fs::read_dir(nooka_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut versions = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with(APP_PREFIX) {
                let version_str = &name[APP_PREFIX.len()..];
                if let Ok(v) = Version::parse(version_str) {
                    versions.push((v, path));
                }
            }
        }
    }

    versions.sort_by(|a, b| a.0.cmp(&b.0));
    versions
}

fn check_for_update(logger: &mut Logger, nooka_dir: &Path) -> Result<Option<GitHubRelease>, String> {
    logger.log("Checking for updates...");

    let output = Command::new(system32_tool("curl.exe"))
        .args(&[
            "-sS",
            "--fail",
            "--max-time",
            "20",
            "--user-agent",
            "Nooka-Updater",
            &releases_url(),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to run curl: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "curl failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let release: GitHubRelease = serde_json::from_str(&json_str)
        .map_err(|e| format!("Failed to parse release: {}", e))?;

    let current_version = get_current_version(nooka_dir);
    let latest_version = Version::parse(release.tag_name.trim_start_matches('v'))
        .map_err(|e| format!("Invalid version format: {}", e))?;

    logger.log(&format!("Current version: {:?}", current_version));
    logger.log(&format!("Latest version: {}", latest_version));

    if current_version.as_ref() < Some(&latest_version) {
        logger.log("Update available!");
        Ok(Some(release))
    } else {
        logger.log("No update available");
        Ok(None)
    }
}

fn download_update(
    logger: &mut Logger,
    release: &GitHubRelease,
    nooka_dir: &Path,
    state: Option<&UiState>,
) -> Result<PathBuf, String> {
    logger.log("Downloading update...");

    // Prefer the Squirrel-style `.nupkg` asset; fall back to `.zip` if that's what was published.
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(".nupkg"))
        .or_else(|| release.assets.iter().find(|a| a.name.ends_with(".zip")))
        .ok_or("No .nupkg or .zip asset found in release")?;

    logger.log(&format!("Downloading: {} ({} bytes)", asset.name, asset.size));

    let temp_dir = nooka_dir.join(".update-temp");
    fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp dir: {}", e))?;

    // El archivo se guarda con extensión `.zip` aunque el asset sea `.nupkg`: `tar.exe` la
    // sniffa por contenido, pero el nombre corto es el que esperan las herramientas de Squirrel.
    let download_path = temp_dir.join("update.zip");
    let _ = fs::remove_file(&download_path);

    let mut child = Command::new(system32_tool("curl.exe"))
        .arg("-sS")
        .arg("--fail")
        .arg("--location")
        .arg("--user-agent")
        .arg("Nooka-Updater")
        .arg("--output")
        .arg(&download_path)
        .arg(&asset.browser_download_url)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start download: {}", e))?;

    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if let Some(state) = state {
                    if let Ok(meta) = fs::metadata(&download_path) {
                        let pct = if asset.size > 0 {
                            ((meta.len() * 100) / asset.size).min(99) as usize
                        } else {
                            0
                        };
                        state.percent.store(pct, Ordering::Relaxed);
                        set_status(state, &format!("Descargando actualización... {}%", pct));
                    }
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("Download process error: {}", e)),
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to collect download output: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Download failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let metadata = fs::metadata(&download_path)
        .map_err(|e| format!("Failed to read downloaded file: {}", e))?;

    if asset.size > 0 && metadata.len() != asset.size {
        return Err(format!(
            "Download size mismatch: expected {} bytes, got {}",
            asset.size,
            metadata.len()
        ));
    }

    if let Some(state) = state {
        state.percent.store(100, Ordering::Relaxed);
        set_status(state, "Descarga completada");
    }

    logger.log(&format!("Downloaded successfully: {:?}", download_path));
    Ok(download_path)
}

fn install_update(logger: &mut Logger, download_path: &Path, nooka_dir: &Path) -> Result<Version, String> {
    logger.log("Installing update...");

    let staging_dir = nooka_dir.join(".staging");

    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir)
            .map_err(|e| format!("Failed to clean staging dir: {}", e))?;
    }

    fs::create_dir_all(&staging_dir)
        .map_err(|e| format!("Failed to create staging dir: {}", e))?;

    logger.log("Extracting update...");
    let output = Command::new(system32_tool("tar.exe"))
        .arg("-xf")
        .arg(download_path)
        .arg("-C")
        .arg(&staging_dir)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("Failed to extract: {}", e))?;

    if !output.status.success() {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(format!(
            "Extraction failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let entries = fs::read_dir(&staging_dir)
        .map_err(|e| format!("Failed to read staging dir: {}", e))?;

    let mut new_version: Option<Version> = None;
    let mut staged_app_dir: Option<PathBuf> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with(APP_PREFIX) {
                    let version_str = &name[APP_PREFIX.len()..];
                    if let Ok(v) = Version::parse(version_str) {
                        new_version = Some(v);
                        staged_app_dir = Some(path);
                        break;
                    }
                }
            }
        }
    }

    let new_version = new_version.ok_or("No valid app version found in update")?;
    let staged_app_dir = staged_app_dir.ok_or("No app directory found in staging")?;

    logger.log(&format!("Extracted version: {}", new_version));

    let nooka_exe = staged_app_dir.join("Nooka.exe");
    if !nooka_exe.exists() {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err("Extracted app is missing Nooka.exe".to_string());
    }

    let target_dir = nooka_dir.join(format!("{}{}", APP_PREFIX, new_version));

    if target_dir.exists() {
        logger.log("Removing existing target directory...");
        fs::remove_dir_all(&target_dir)
            .map_err(|e| format!("Failed to remove existing version: {}", e))?;
    }

    logger.log(&format!("Moving to final location: {:?}", target_dir));
    fs::rename(&staged_app_dir, &target_dir)
        .map_err(|e| format!("Failed to move new version: {}", e))?;

    refresh_root_binaries(logger, &staging_dir, nooka_dir);

    let _ = fs::remove_dir_all(&staging_dir);

    logger.log(&format!("Update installed successfully: {}", new_version));
    Ok(new_version)
}

// El `.nupkg` del release trae tambien `Update.exe` y `Nooka.exe` en su raiz. Si aqui no se
// copian, quien venia de una version vieja pasa a la app nueva pero sigue arrancando para
// siempre con el launcher y el updater antiguos: los cambios de estos binarios no llegarian
// a nadie que no instale desde cero.
//
// El exe en marcha no se puede borrar, pero si MOVER (la seccion de imagen se abre con
// FILE_SHARE_DELETE), de ahi el `.old` provisional. Si copiar el nuevo falla se deshace el
// movimiento; si tiene exito se intenta borrar el `.old` y, si aun esta bloqueado, se va
// en la siguiente pasada desde el binario que si se instalo.
fn refresh_root_binaries(logger: &mut Logger, staging_dir: &Path, nooka_dir: &Path) {
    for name in ["Update.exe", "Nooka.exe"] {
        let staged = staging_dir.join(name);
        if !staged.exists() {
            logger.log(&format!("Raiz {}: el paquete no la trae, se deja la que hay", name));
            continue;
        }

        let dest = nooka_dir.join(name);
        let old = nooka_dir.join(format!("{}.old", name));

        if dest.exists() {
            if let Err(e) = fs::rename(&dest, &old) {
                logger.log(&format!("Raiz {}: no se pudo apartar la version vieja ({}); se deja como esta", name, e));
                continue;
            }
        }

        match fs::copy(&staged, &dest) {
            Ok(_) => {
                logger.log(&format!("Raiz {} actualizada", name));
                // El `.old` de la imagen que esta en marcha NO se puede borrar todavia; se va
                // solo la proxima vez, cuando quien corre ya es el binario nuevo.
                if old.exists() && fs::remove_file(&old).is_err() {
                    logger.log(&format!("Raiz {}: {}.old queda pendiente para el proximo arranque", name, name));
                }
            }
            Err(e) => {
                // Volver a mover el `.old` restaura el arranque, aunque el borrado posterior
                // haya dejado la raiz en dos tiempos.
                if old.exists() {
                    let _ = fs::rename(&old, &dest);
                }
                logger.log(&format!("Raiz {}: no se pudo instalar ({})", name, e));
            }
        }
    }
}

fn cleanup_old_versions(logger: &mut Logger, nooka_dir: &Path) -> Result<(), String> {
    logger.log("Cleaning up old versions...");

    let versions = get_all_versions(nooka_dir);

    if versions.len() <= MAX_OLD_VERSIONS {
        logger.log(&format!("Only {} versions present, no cleanup needed", versions.len()));
        return Ok(());
    }

    let to_remove = versions.len() - MAX_OLD_VERSIONS;
    logger.log(&format!("Removing {} old version(s)...", to_remove));

    for (version, path) in versions.iter().take(to_remove) {
        logger.log(&format!("Removing version {}: {:?}", version, path));
        if let Err(e) = fs::remove_dir_all(path) {
            logger.log(&format!("Warning: Failed to remove old version: {}", e));
        }
    }

    Ok(())
}

fn cleanup_temp_files(logger: &mut Logger, nooka_dir: &Path) {
    logger.log("Cleaning up temporary files...");

    let temp_dir = nooka_dir.join(".update-temp");
    if temp_dir.exists() {
        if let Err(e) = fs::remove_dir_all(&temp_dir) {
            logger.log(&format!("Warning: Failed to clean temp dir: {}", e));
        }
    }

    let staging_dir = nooka_dir.join(".staging");
    if staging_dir.exists() {
        if let Err(e) = fs::remove_dir_all(&staging_dir) {
            logger.log(&format!("Warning: Failed to clean staging dir: {}", e));
        }
    }
}

fn create_shortcut(logger: &mut Logger, nooka_dir: &Path) -> Result<(), String> {
    logger.log("Creating shortcuts...");

    let versions = get_all_versions(nooka_dir);
    let (_, app_dir) = versions.last().ok_or("No app version found")?;

    let nooka_exe = app_dir.join("Nooka.exe");
    let update_exe = nooka_dir.join("Update.exe");

    let desktop = dirs::desktop_dir().ok_or("Could not determine desktop directory")?;
    let desktop_shortcut = desktop.join("Nooka.lnk");

    let start_menu = dirs::data_dir()
        .ok_or("Could not determine app data directory")?
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");
    fs::create_dir_all(&start_menu)
        .map_err(|e| format!("Failed to create start menu dir: {}", e))?;
    let start_shortcut = start_menu.join("Nooka.lnk");

    unsafe {
        write_shortcut(
            &desktop_shortcut,
            &update_exe,
            "--processStart Nooka.exe",
            nooka_dir,
            &nooka_exe,
            "Nooka",
        )?;
        write_shortcut(
            &start_shortcut,
            &update_exe,
            "--processStart Nooka.exe",
            nooka_dir,
            &nooka_exe,
            "Nooka",
        )?;
    }

    logger.log("Shortcuts created successfully");
    Ok(())
}

fn system32_tool(name: &str) -> PathBuf {
    // Se resuelve por %SystemRoot% y no por PATH: un curl.exe o un tar.exe ajeno metido antes
    // en el PATH nos entregaría la respuesta de GitHub o lo que él quisiera descomprimir.
    let root = env::var("SystemRoot").unwrap_or_else(|_| String::from(r"C:\Windows"));
    Path::new(&root).join("System32").join(name)
}

// El .lnk se escribe por COM (IShellLinkW + IPersistFile). Antes era un
// `New-Object -ComObject WScript.Shell` dentro de un powershell.exe hijo del updater, y esa
// cadena —intérprete de scripts que baja un exe, lo descomprime y le crea un acceso
// directo— es justo la forma de un dropper: dos motores la marcaban como malware genérico.
unsafe fn write_shortcut(
    lnk: &Path,
    target: &Path,
    arguments: &str,
    working_dir: &Path,
    icon: &Path,
    description: &str,
) -> Result<(), String> {
    let wide = |s: &str| U16CString::from_str(s).map_err(|e| e.to_string());
    let target_w = wide(&target.to_string_lossy())?;
    let args_w = wide(arguments)?;
    let dir_w = wide(&working_dir.to_string_lossy())?;
    // La ruta del icono va PELADA y el índice por su propio argumento: meter ",0" dentro de la
    // ruta (lo que hace la sintaxis de WScript.Shell) guarda un archivo inexistente.
    let icon_w = wide(&icon.to_string_lossy())?;
    let desc_w = wide(description)?;
    let lnk_w = wide(&lnk.to_string_lossy())?;

    let co = CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED);
    let mut link: *mut IShellLinkW = ptr::null_mut();
    let iid_link = IShellLinkW::uuidof();
    let created = CoCreateInstance(
        &CLSID_SHELL_LINK,
        ptr::null_mut(),
        CLSCTX_INPROC_SERVER,
        &iid_link,
        &mut link as *mut _ as *mut *mut c_void,
    );
    if created != S_OK || link.is_null() {
        if co == S_OK {
            CoUninitialize();
        }
        return Err(format!("CoCreateInstance(ShellLink) -> 0x{:08X}", created));
    }

    let shell_link = &*link;
    let mut hr = shell_link.SetPath(target_w.as_ptr());
    if hr == S_OK {
        hr = shell_link.SetArguments(args_w.as_ptr());
    }
    if hr == S_OK {
        hr = shell_link.SetWorkingDirectory(dir_w.as_ptr());
    }
    if hr == S_OK {
        hr = shell_link.SetIconLocation(icon_w.as_ptr(), 0);
    }
    if hr == S_OK {
        hr = shell_link.SetDescription(desc_w.as_ptr());
    }
    if hr == S_OK {
        let mut persist: *mut IPersistFile = ptr::null_mut();
        let iid_file = IPersistFile::uuidof();
        hr = shell_link.QueryInterface(&iid_file, &mut persist as *mut _ as *mut *mut c_void);
        if hr == S_OK {
            hr = (*persist).Save(lnk_w.as_ptr(), FALSE);
            (*persist).Release();
        }
    }
    (*link).Release();
    if co == S_OK {
        CoUninitialize();
    }

    if hr != S_OK {
        return Err(format!("shortcut {} -> 0x{:08X}", lnk.display(), hr));
    }
    Ok(())
}

fn remove_shortcut(logger: &mut Logger) -> Result<(), String> {
    logger.log("Removing shortcuts...");

    let desktop = dirs::desktop_dir().ok_or("Could not determine desktop directory")?;
    let desktop_shortcut = desktop.join("Nooka.lnk");

    let start_menu = dirs::data_dir()
        .ok_or("Could not determine app data directory")?
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join("Nooka.lnk");

    if desktop_shortcut.exists() {
        fs::remove_file(&desktop_shortcut)
            .map_err(|e| format!("Failed to remove desktop shortcut: {}", e))?;
    }

    if start_menu.exists() {
        fs::remove_file(&start_menu)
            .map_err(|e| format!("Failed to remove start menu shortcut: {}", e))?;
    }

    logger.log("Shortcuts removed successfully");
    Ok(())
}

fn launch_latest_app(logger: &mut Logger, nooka_dir: &Path) -> Result<(), String> {
    let versions = get_all_versions(nooka_dir);
    let (_, app_dir) = versions.last().ok_or("No app version found")?;

    let nooka_exe = app_dir.join("Nooka.exe");

    Command::new(&nooka_exe)
        .current_dir(app_dir)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Failed to launch Nooka: {}", e))?;

    logger.log("Nooka launched successfully");
    Ok(())
}

fn launch_app(logger: &mut Logger, nooka_dir: &Path) -> Result<(), String> {
    logger.log("Launching Nooka...");

    // Prefer the root launcher (splash + update check) when it exists
    let root_launcher = nooka_dir.join("Nooka.exe");
    if root_launcher.exists() {
        Command::new(&root_launcher)
            .current_dir(nooka_dir)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Failed to launch Nooka: {}", e))?;
        logger.log("Nooka launcher started");
        return Ok(());
    }

    launch_latest_app(logger, nooka_dir)
}

fn run_update(state: &UiState, nooka_dir: &Path, logger: &mut Logger) -> Result<(), String> {
    let release = match check_for_update(logger, nooka_dir) {
        Ok(r) => r,
        Err(e) => {
            // Network/API errors are not fatal: keep the current version, like Discord
            logger.log(&format!("Could not check for updates: {}", e));
            return Ok(());
        }
    };

    let release = match release {
        Some(r) => r,
        None => return Ok(()),
    };

    let path = download_update(logger, &release, nooka_dir, Some(state))?;

    set_status(state, "Instalando actualización...");
    state.percent.store(MARQUEE, Ordering::Relaxed);

    match install_update(logger, &path, nooka_dir) {
        Ok(_version) => {
            let _ = cleanup_old_versions(logger, nooka_dir);
            cleanup_temp_files(logger, nooka_dir);
            let _ = create_shortcut(logger, nooka_dir);
            let _ = launch_latest_app(logger, nooka_dir);
            Ok(())
        }
        Err(e) => {
            cleanup_temp_files(logger, nooka_dir);
            Err(e)
        }
    }
}

fn main() -> ExitCode {
    // El Subsistema=2 (consola) hace que Windows regale una ventana de terminal
    // al arrancar desde el atajo. La ocultamos sólo si NO la estamos heredando
    // de un cmd/powershell (contando los procesos adjuntos a la consola).
    unsafe {
        let hwnd = GetConsoleWindow();
        if !hwnd.is_null() {
            let mut pids = [0u32; 8];
            let count = GetConsoleProcessList(pids.as_mut_ptr(), pids.len() as u32);
            if count <= 1 {
                ShowWindow(hwnd, SW_HIDE);
            }
        }
    }

    let config = UpdaterConfig::from_args();
    let mut logger = Logger::new(&config.nooka_dir);

    logger.log(&format!("Command: {}", config.command));
    logger.log(&format!("Nooka directory: {:?}", config.nooka_dir));

    // El `.old` que deja una actualizacion esta bloqueado mientras vive el proceso que se
    // aparto a si mismo; quien lo puede borrar es la siguiente arrancada, que ya es el
    // binario nuevo.
    for name in ["Update.exe.old", "Nooka.exe.old"] {
        let leftover = config.nooka_dir.join(name);
        if leftover.exists() {
            let _ = fs::remove_file(&leftover);
        }
    }

    let result = match config.command.as_str() {
        "--update" => run_update_with_ui(&config.nooka_dir),
        "--checkForUpdate" => match check_for_update(&mut logger, &config.nooka_dir) {
            Ok(Some(release)) => {
                logger.log(&format!("Update available: {}", release.tag_name));
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        },
        "--download" => match check_for_update(&mut logger, &config.nooka_dir) {
            Ok(Some(release)) => download_update(&mut logger, &release, &config.nooka_dir, None).map(|_| ()),
            Ok(None) => {
                logger.log("No update to download");
                Ok(())
            }
            Err(e) => Err(e),
        },
        "--createShortcut" => create_shortcut(&mut logger, &config.nooka_dir),
        "--removeShortcut" => remove_shortcut(&mut logger),
        "--processStart" => launch_app(&mut logger, &config.nooka_dir),
        _ => {
            logger.log("Usage: Update.exe [--checkForUpdate|--download|--update|--createShortcut|--removeShortcut|--processStart]");
            Ok(())
        }
    };

    match result {
        Ok(()) => {
            logger.log("Operation completed successfully");
            ExitCode::SUCCESS
        }
        Err(e) => {
            logger.log(&format!("Error: {}", e));
            show_error_box(&e);
            ExitCode::FAILURE
        }
    }
}
