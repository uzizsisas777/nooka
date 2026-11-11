#![windows_subsystem = "windows"]

use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use widestring::U16CString;
use winapi::ctypes::c_void;
use winapi::shared::guiddef::GUID;
use winapi::shared::minwindef::{DWORD, FALSE, LPARAM, LRESULT, WPARAM};
use winapi::shared::windef::{HDC, HWND, RECT};
use winapi::shared::winerror::S_OK;
use winapi::um::combaseapi::{CoCreateInstance, CoInitializeEx, CoUninitialize};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::objidl::IPersistFile;
use winapi::um::shobjidl_core::IShellLinkW;
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
use winapi::Interface;

const NOOKA_NUPKG: &[u8] = include_bytes!("../nooka-full.nupkg");
const UPDATE_EXE: &[u8] = include_bytes!("../Update.exe");
const LAUNCHER_EXE: &[u8] = include_bytes!("../Nooka.exe");
const ICON_ICO: &[u8] = include_bytes!("../icon.ico");

const WINDOW_CLASS: &str = "NookaSetupProgress";
const WM_SETUP_DONE: u32 = WM_APP + 1;

static ICON_PATH: OnceLock<PathBuf> = OnceLock::new();

fn get_install_dir() -> PathBuf {
    let local_app_data = env::var("LOCALAPPDATA")
        .unwrap_or_else(|_| "C:\\Users\\Default\\AppData\\Local".to_string());
    PathBuf::from(local_app_data).join("Nooka")
}

fn show_error_box(message: &str) {
    let text = U16CString::from_str(message).unwrap_or_default();
    let title = U16CString::from_str("Nooka Setup").unwrap_or_default();
    unsafe {
        MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR);
    }
}

struct Logger {
    log_file: Option<fs::File>,
}

impl Logger {
    fn new(install_dir: &Path) -> Self {
        let log_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(install_dir.join("SquirrelSetup.log"))
            .ok();
        Self { log_file }
    }

    fn log(&mut self, message: &str) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let line = format!("[{}] [Setup] {}\n", timestamp, message);
        if let Some(ref mut file) = self.log_file {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }
}

// ---------- Progress UI ----------

struct UiState {
    status: Mutex<String>,
    result: Mutex<Option<Result<(), String>>>,
}

static UI: OnceLock<Arc<UiState>> = OnceLock::new();

fn set_status(state: &UiState, text: &str) {
    if let Ok(mut status) = state.status.lock() {
        *status = text.to_string();
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

    if let Some(icon_path) = ICON_PATH.get() {
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
    }

    let title_rect = RECT { left: 64, top: 18, right: 400, bottom: 42 };
    draw_text(hdc, "Instalando Nooka", &title_rect, 20, 700, RGB(255, 255, 255));

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

    // Marquee track (extraction has no measurable percentage)
    let track = RECT { left: 20, top: 84, right: 400, bottom: 92 };
    let track_brush = CreateSolidBrush(RGB(64, 68, 75));
    FillRect(hdc, &track, track_brush);
    DeleteObject(track_brush as *mut _);

    let fill_brush = CreateSolidBrush(RGB(88, 101, 242));
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
        WM_SETUP_DONE => {
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
        let window_name = U16CString::from_str("Instalando Nooka").ok()?;

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

fn run_install_with_ui(install_dir: &Path) -> Result<(), String> {
    let state = Arc::new(UiState {
        status: Mutex::new("Preparando instalación...".to_string()),
        result: Mutex::new(None),
    });
    let _ = UI.set(state.clone());

    let hwnd = create_progress_window().ok_or("Could not create progress window")?;
    let hwnd_raw = hwnd as isize;
    let worker_state = state.clone();
    let dir = install_dir.to_path_buf();

    thread::spawn(move || {
        let result = run_install(&worker_state, &dir);
        if let Ok(mut slot) = worker_state.result.lock() {
            *slot = Some(result);
        }
        unsafe {
            PostMessageW(hwnd_raw as HWND, WM_SETUP_DONE, 0, 0);
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

// ---------- Install logic ----------

fn nupkg_version(nupkg_dir: &Path) -> String {
    let nuspec = nupkg_dir.join("Nooka.nuspec");
    if let Ok(content) = fs::read_to_string(&nuspec) {
        if let Some(start) = content.find("<version>") {
            let rest = &content[start + "<version>".len()..];
            if let Some(end) = rest.find("</version>") {
                return rest[..end].trim().to_string();
            }
        }
    }
    "1.0.0".to_string()
}

fn extract_nupkg(logger: &mut Logger, install_dir: &Path) -> io::Result<()> {
    logger.log(&format!("Extracting Nooka to {:?}", install_dir));

    fs::create_dir_all(install_dir)?;

    // bsdtar mira la firma del archivo, no la extensión, así que el .nupkg se descomprime
    // directamente sin copiarlo a un falso .zip.
    let temp_zip = install_dir.join("nooka-temp.nupkg");
    fs::write(&temp_zip, NOOKA_NUPKG)?;

    let staging = install_dir.join(".setup-staging");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    fs::create_dir_all(&staging)?;

    // `tar.exe` de System32 (bsdtar) en vez de `powershell -Command Expand-Archive`: un
    // intérprete de scripts que suelta un exe dentro de AppData es la forma exacta de un
    // dropper, y dos motores antivirus nos marcaban por ella.
    let output = Command::new(system32_tool("tar.exe"))
        .args(&["-xf", &temp_zip.to_string_lossy(), "-C", &staging.to_string_lossy()])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;

    let _ = fs::remove_file(&temp_zip);

    if !output.status.success() {
        let _ = fs::remove_dir_all(&staging);
        logger.log(&format!("Extraction failed: {}", String::from_utf8_lossy(&output.stderr)));
        return Err(io::Error::new(io::ErrorKind::Other, "Failed to extract package"));
    }

    // The .nupkg uses NuGet layout (lib/net45); convert to Squirrel layout (app-X.Y.Z)
    let version = nupkg_version(&staging);
    let app_files = staging.join("lib").join("net45");
    if !app_files.join("Nooka.exe").exists() {
        let _ = fs::remove_dir_all(&staging);
        return Err(io::Error::new(io::ErrorKind::NotFound, "Package is missing Nooka.exe"));
    }

    let target = install_dir.join(format!("app-{}", version));
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    fs::rename(&app_files, &target)?;
    let _ = fs::remove_dir_all(&staging);

    // Root files: updater, launcher (splash), icon
    fs::write(install_dir.join("Update.exe"), UPDATE_EXE)?;
    fs::write(install_dir.join("Nooka.exe"), LAUNCHER_EXE)?;
    fs::write(install_dir.join("icon.ico"), ICON_ICO)?;

    logger.log(&format!("Installed app-{} successfully", version));
    Ok(())
}

fn system32_tool(name: &str) -> PathBuf {
    // Se resuelve por %SystemRoot% y no por PATH: un tar.exe ajeno metido antes en el PATH
    // decidiría qué se descomprime y dónde.
    let root = env::var("SystemRoot").unwrap_or_else(|_| String::from(r"C:\Windows"));
    Path::new(&root).join("System32").join(name)
}

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

fn create_shortcut(logger: &mut Logger, install_dir: &Path) -> io::Result<()> {
    logger.log("Creating shortcuts...");

    let desktop = dirs::desktop_dir().unwrap_or_else(|| PathBuf::from("C:\\Users\\Public\\Desktop"));
    let desktop_shortcut = desktop.join("Nooka.lnk");

    let start_menu = dirs::data_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No app data dir"))?
        .join("Microsoft").join("Windows").join("Start Menu").join("Programs");
    fs::create_dir_all(&start_menu)?;
    let start_shortcut = start_menu.join("Nooka.lnk");

    let update_exe = install_dir.join("Update.exe");
    let icon_path = install_dir.join("icon.ico");

    // El .lnk se escribe por COM (IShellLinkW + IPersistFile). Antes era un
    // `New-Object -ComObject WScript.Shell` dentro de un powershell.exe hijo del setup, y esa
    // cadena —intérprete de scripts que suelta un exe en AppData y le crea su acceso
    // directo— es justo la forma de un dropper: dos motores la marcaban como malware genérico.
    unsafe {
        write_shortcut(
            &desktop_shortcut,
            &update_exe,
            "--processStart Nooka.exe",
            install_dir,
            &icon_path,
            "Nooka",
        )
        .map_err(io::Error::other)?;
        write_shortcut(
            &start_shortcut,
            &update_exe,
            "--processStart Nooka.exe",
            install_dir,
            &icon_path,
            "Nooka",
        )
        .map_err(io::Error::other)?;
    }

    logger.log("Shortcuts created");
    Ok(())
}

fn launch_nooka(install_dir: &Path) -> io::Result<()> {
    let launcher = install_dir.join("Nooka.exe");
    Command::new(&launcher)
        .current_dir(install_dir)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(())
}

fn run_install(state: &UiState, install_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(install_dir)
        .map_err(|e| format!("Failed to create install directory: {}", e))?;
    let mut logger = Logger::new(install_dir);
    logger.log("Nooka Setup started");

    set_status(state, "Extrayendo archivos de Nooka...");
    extract_nupkg(&mut logger, install_dir)
        .map_err(|e| format!("Failed to install Nooka: {}", e))?;

    set_status(state, "Creando accesos directos...");
    if let Err(e) = create_shortcut(&mut logger, install_dir) {
        logger.log(&format!("Warning: failed to create shortcuts: {}", e));
    }

    set_status(state, "Iniciando Nooka...");
    launch_nooka(install_dir).map_err(|e| format!("Failed to launch Nooka: {}", e))?;

    logger.log("Nooka installed successfully");
    Ok(())
}

fn main() -> ExitCode {
    // Ocultar la consola que Windows regala por el subsistema=2 sólo cuando no
    // la heredamos de un cmd/powershell (el setup descargado no debe enseñar terminal).
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

    // The setup may run from anywhere (Downloads); unpack the embedded icon for the progress window
    let icon_temp = env::temp_dir().join("nooka-setup-icon.ico");
    if fs::write(&icon_temp, ICON_ICO).is_ok() {
        let _ = ICON_PATH.set(icon_temp);
    }

    let install_dir = get_install_dir();

    let result = if install_dir.join("Update.exe").exists() && install_dir.join("Nooka.exe").exists() {
        launch_nooka(&install_dir).map_err(|e| format!("Failed to launch Nooka: {}", e))
    } else {
        run_install_with_ui(&install_dir)
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            show_error_box(&e);
            ExitCode::FAILURE
        }
    }
}