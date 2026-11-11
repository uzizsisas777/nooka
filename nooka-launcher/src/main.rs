#![windows_subsystem = "windows"]

use semver::Version;
use serde::Deserialize;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};
use widestring::U16CString;
use winapi::shared::minwindef::{LPARAM, LRESULT, WPARAM};
use winapi::shared::windef::{HWND, RECT};
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winbase::CREATE_NO_WINDOW;
use winapi::um::wincon::{GetConsoleProcessList, GetConsoleWindow};
use winapi::um::wingdi::{CreateSolidBrush, RGB};
use winapi::um::winuser::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, DrawIconEx,
    FillRect, GetMessageW, LoadCursorW, LoadImageW, PostQuitMessage, RegisterClassExW,
    ShowWindow, TranslateMessage, UpdateWindow, IDC_ARROW, IMAGE_ICON, LR_LOADFROMFILE,
    SW_HIDE, SW_SHOW, WM_CREATE, WM_DESTROY, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_TOPMOST,
    WS_POPUP, WS_VISIBLE,
};

const NOOKA_EXE: &str = "Nooka.exe";
const APP_PREFIX: &str = "app-";
const LOG_FILE: &str = "nooka_launcher.log";
const WINDOW_CLASS: &str = "NookaLauncherClass";
const SPLASH_DURATION_MS: u32 = 1500;
const ICON_SIZE: i32 = 128;
const ICON_NAME: &str = "NOOKA_ICON";
const RELEASES_URL: &str = "https://api.github.com/repos/uzizsisas777/nooka/releases/latest";

#[derive(Deserialize, Debug)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize, Debug)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

struct LauncherConfig {
    nooka_dir: PathBuf,
    process_start: Option<String>,
    process_start_and_wait: bool,
    process_start_args: Vec<String>,
    no_ui: bool,
}

impl LauncherConfig {
    fn from_args() -> Self {
        let args: Vec<String> = env::args().collect();
        let exe_path = env::current_exe().unwrap_or_default();
        let nooka_dir = exe_path.parent().unwrap_or(Path::new(".")).to_path_buf();

        let mut config = LauncherConfig {
            nooka_dir,
            process_start: None,
            process_start_and_wait: false,
            process_start_args: Vec::new(),
            no_ui: false,
        };

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--processStart" => {
                    if i + 1 < args.len() {
                        config.process_start = Some(args[i + 1].clone());
                        i += 1;
                    }
                }
                "--processStartAndWait" => {
                    config.process_start_and_wait = true;
                }
                "--process-start-args" => {
                    if i + 1 < args.len() {
                        config.process_start_args = args[i + 1..].to_vec();
                        break;
                    }
                }
                "--no-ui" => {
                    config.no_ui = true;
                }
                _ => {}
            }
            i += 1;
        }

        config
    }
}

struct SimpleLogger {
    log_file: PathBuf,
}

impl SimpleLogger {
    fn new(nooka_dir: &Path) -> Self {
        SimpleLogger {
            log_file: nooka_dir.join(LOG_FILE),
        }
    }

    fn log(&self, level: &str, message: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        
        let log_line = format!("[{}] {} - {}\n", now.as_secs(), level, message);
        
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file)
        {
            let _ = file.write_all(log_line.as_bytes());
        }
    }

    fn info(&self, message: &str) {
        self.log("INFO", message);
    }

    fn error(&self, message: &str) {
        self.log("ERROR", message);
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            // Start timer for splash screen
            unsafe {
                winapi::um::winuser::SetTimer(hwnd, 1, SPLASH_DURATION_MS, None);
            }
            0
        }
        WM_TIMER => {
            // Timer fired, close the splash and launch the app
            unsafe {
                winapi::um::winuser::KillTimer(hwnd, 1);
                PostQuitMessage(0);
            }
            0
        }
        WM_PAINT => {
            unsafe {
                let mut ps: winapi::um::winuser::PAINTSTRUCT = std::mem::zeroed();
                let hdc = winapi::um::winuser::BeginPaint(hwnd, &mut ps);
                
                let mut rect: RECT = std::mem::zeroed();
                winapi::um::winuser::GetClientRect(hwnd, &mut rect);
                let client_w = rect.right - rect.left;
                let client_h = rect.bottom - rect.top;
                
                // Fondo oscuro
                let brush = CreateSolidBrush(RGB(26, 26, 26));
                FillRect(hdc, &rect, brush);
                winapi::um::wingdi::DeleteObject(brush as *mut _);
                
                // Cargar ícono desde el archivo
                let exe_path = env::current_exe().unwrap_or_default();
                let icon_path = exe_path.parent()
                    .unwrap_or(Path::new("."))
                    .join("icon.ico");
                
                let mut icon_y_offset = 0;
                
                if let Ok(icon_path_str) = icon_path.into_os_string().into_string() {
                    if let Ok(icon_wide) = U16CString::from_str(&icon_path_str) {
                        let hicon = LoadImageW(
                            std::ptr::null_mut(),
                            icon_wide.as_ptr(),
                            IMAGE_ICON,
                            ICON_SIZE,
                            ICON_SIZE,
                            LR_LOADFROMFILE,
                        );
                        
                        if !hicon.is_null() {
                            let icon_x = (client_w - ICON_SIZE) / 2;
                            let icon_y = 40; // Posición fija arriba
                            icon_y_offset = ICON_SIZE + 20;
                            
                            // Dibujar con fondo blanco para que no sea transparente
                            let bg_brush = CreateSolidBrush(RGB(255, 255, 255));
                            let icon_rect = RECT {
                                left: icon_x,
                                top: icon_y,
                                right: icon_x + ICON_SIZE,
                                bottom: icon_y + ICON_SIZE,
                            };
                            FillRect(hdc, &icon_rect, bg_brush);
                            winapi::um::wingdi::DeleteObject(bg_brush as *mut _);
                            
                            DrawIconEx(
                                hdc,
                                icon_x,
                                icon_y,
                                hicon as *mut _,
                                ICON_SIZE,
                                ICON_SIZE,
                                0,
                                std::ptr::null_mut(),
                                0x0003, // DI_IMAGE | DI_MASK
                            );
                            
                            winapi::um::winuser::DestroyIcon(hicon as *mut _);
                        }
                    }
                }
                
                let text = U16CString::from_str("Nooka").unwrap();
                winapi::um::wingdi::SetTextColor(hdc, RGB(255, 255, 255));
                winapi::um::wingdi::SetBkMode(hdc, winapi::um::wingdi::TRANSPARENT as i32);
                
                let font = winapi::um::wingdi::CreateFontW(
                    48, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 0, 0,
                    U16CString::from_str("Segoe UI").unwrap().as_ptr(),
                );
                
                let old_font = winapi::um::wingdi::SelectObject(hdc, font as *mut _);
                
                let text_height = 60;
                let text_rect = RECT {
                    left: 0,
                    top: icon_y_offset + 20,
                    right: client_w,
                    bottom: icon_y_offset + 20 + text_height,
                };
                
                winapi::um::winuser::DrawTextW(
                    hdc,
                    text.as_ptr(),
                    -1,
                    &text_rect as *const _ as *mut _,
                    winapi::um::winuser::DT_CENTER | winapi::um::winuser::DT_VCENTER | winapi::um::winuser::DT_SINGLELINE,
                );
                
                winapi::um::wingdi::SelectObject(hdc, old_font as *mut _);
                winapi::um::wingdi::DeleteObject(font as *mut _);
                
                winapi::um::winuser::EndPaint(hwnd, &ps);
            }
            0
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) },
    }
}

fn create_splash_window() -> Option<HWND> {
    unsafe {
        let class_name = U16CString::from_str(WINDOW_CLASS).unwrap();
        let window_name = U16CString::from_str("Nooka").unwrap();
        
        let mut wcex: WNDCLASSEXW = std::mem::zeroed();
        wcex.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wcex.lpfnWndProc = Some(window_proc);
        wcex.hInstance = GetModuleHandleW(std::ptr::null());
        wcex.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_ARROW);
        wcex.lpszClassName = class_name.as_ptr();
        
        if RegisterClassExW(&wcex) == 0 {
            return None;
        }
        
        let screen_width = winapi::um::winuser::GetSystemMetrics(winapi::um::winuser::SM_CXSCREEN);
        let screen_height = winapi::um::winuser::GetSystemMetrics(winapi::um::winuser::SM_CYSCREEN);
        
        let window_width = 400;
        let window_height = 300;
        let x = (screen_width - window_width) / 2;
        let y = (screen_height - window_height) / 2;
        
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST,
            class_name.as_ptr(),
            window_name.as_ptr(),
            WS_POPUP | WS_VISIBLE,  // Sin titlebar, solo ventana popup
            x,
            y,
            window_width,
            window_height,
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

fn run_message_loop(_hwnd: HWND) {
    unsafe {
        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn find_app_versions(nooka_dir: &Path, logger: &SimpleLogger) -> Vec<(Version, PathBuf)> {
    let mut versions = Vec::new();

    let entries = match fs::read_dir(nooka_dir) {
        Ok(e) => e,
        Err(e) => {
            logger.error(&format!("Failed to read directory {:?}: {}", nooka_dir, e));
            return versions;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if !name.starts_with(APP_PREFIX) {
            continue;
        }

        let version_str = &name[APP_PREFIX.len()..];
        match Version::parse(version_str) {
            Ok(v) => {
                logger.info(&format!("Found version {} at {:?}", v, path));
                versions.push((v, path));
            }
            Err(e) => {
                logger.error(&format!("Invalid version format in {}: {}", name, e));
            }
        }
    }

    versions.sort_by(|a, b| b.0.cmp(&a.0));
    versions
}

fn find_latest_version(nooka_dir: &Path, logger: &SimpleLogger) -> Option<PathBuf> {
    let versions = find_app_versions(nooka_dir, logger);
    versions.into_iter().next().map(|(_, path)| path)
}

fn get_current_version(nooka_dir: &Path) -> Option<Version> {
    let entries = fs::read_dir(nooka_dir).ok()?;
    let mut versions = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = path.file_name()?.to_str()?;
        if !name.starts_with(APP_PREFIX) {
            continue;
        }

        let version_str = &name[APP_PREFIX.len()..];
        if let Ok(v) = Version::parse(version_str) {
            versions.push(v);
        }
    }

    versions.sort();
    versions.into_iter().last()
}

fn system32_tool(name: &str) -> PathBuf {
    // Se resuelve por %SystemRoot% y no por PATH: un curl.exe ajeno metido antes en el PATH
    // nos entregaría la respuesta de GitHub, y ese es justo el camino que un AV vigila.
    let root = env::var("SystemRoot").unwrap_or_else(|_| String::from(r"C:\Windows"));
    Path::new(&root).join("System32").join(name)
}

fn check_for_update(logger: &SimpleLogger) -> Option<GitHubRelease> {
    logger.info("Checking for updates...");

    let output = Command::new(system32_tool("curl.exe"))
        .args(&[
            "-sS",
            "--fail",
            "--max-time",
            "20",
            "--user-agent",
            "Nooka-Updater",
            RELEASES_URL,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    if !output.status.success() {
        logger.error(&format!(
            "curl failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
        return None;
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let release: GitHubRelease = serde_json::from_str(&json_str).ok()?;

    let latest_version = Version::parse(release.tag_name.trim_start_matches('v')).ok()?;
    logger.info(&format!("Latest version on GitHub: {}", latest_version));

    Some(release)
}

fn launch_updater(nooka_dir: &Path, logger: &SimpleLogger) -> Result<(), String> {
    logger.info("Launching updater...");
    
    let updater_path = nooka_dir.join("Update.exe");
    if !updater_path.exists() {
        return Err("Update.exe not found".to_string());
    }

    let mut cmd = Command::new(&updater_path);
    cmd.arg("--update");
    cmd.creation_flags(CREATE_NO_WINDOW);

    let status = cmd.status().map_err(|e| format!("Failed to launch updater: {}", e))?;
    
    if status.success() {
        logger.info("Updater completed successfully");
        Ok(())
    } else {
        Err("Updater failed".to_string())
    }
}

fn launch_process(exe_path: &Path, args: &[String], wait: bool, logger: &SimpleLogger) -> Result<i32, String> {
    logger.info(&format!("Launching: {:?}", exe_path));
    logger.info(&format!("Arguments: {:?}", args));

    let mut cmd = Command::new(exe_path);
    cmd.args(args);

    if !wait {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn process: {}", e))?;

    if wait {
        let status = child.wait().map_err(|e| format!("Failed to wait for process: {}", e))?;
        Ok(status.code().unwrap_or(0))
    } else {
        Ok(0)
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

    let config = LauncherConfig::from_args();
    let logger = SimpleLogger::new(&config.nooka_dir);
    
    let start_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    
    logger.info(&format!("=== Nooka Launcher Started at {} ===", start_time.as_secs()));
    logger.info(&format!("Nooka directory: {:?}", config.nooka_dir));

    // Show splash screen if not in no-ui mode
    if !config.no_ui {
        if let Some(hwnd) = create_splash_window() {
            logger.info("Splash screen shown");
            
            // Run message loop (blocks until timer fires)
            run_message_loop(hwnd);
            
            logger.info("Splash screen closed");
            unsafe { DestroyWindow(hwnd); }
        }
    }

    // Check for updates
    let current_version = get_current_version(&config.nooka_dir);
    logger.info(&format!("Current version: {:?}", current_version));

    if let Some(release) = check_for_update(&logger) {
        let latest_version = Version::parse(release.tag_name.trim_start_matches('v'));
        
        if let Ok(latest) = latest_version {
            if current_version.as_ref() < Some(&latest) {
                logger.info(&format!("Update available: {} -> {}", current_version.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "unknown".to_string()), latest));
                
                // Launch updater; on success fall through so find_latest_version
                // picks up the freshly extracted app-X.Y.Z and launches it.
                match launch_updater(&config.nooka_dir, &logger) {
                    Ok(()) => {
                        logger.info("Update installed, relaunching with new version");
                    }
                    Err(e) => {
                        logger.error(&format!("Update failed: {}", e));
                        // Continue with current version
                    }
                }
            } else {
                logger.info("No update available");
            }
        }
    } else {
        logger.info("Could not check for updates, continuing with current version");
    }

    // Find latest version
    let latest_path = match find_latest_version(&config.nooka_dir, &logger) {
        Some(p) => p,
        None => {
            logger.error(&format!("No app versions found in {:?}", config.nooka_dir));
            return ExitCode::FAILURE;
        }
    };

    logger.info(&format!("Latest version path: {:?}", latest_path));

    // Determine which executable to launch
    let exe_to_launch = if let Some(ref process_name) = config.process_start {
        latest_path.join(process_name)
    } else {
        latest_path.join(NOOKA_EXE)
    };

    if !exe_to_launch.exists() {
        logger.error(&format!("Executable not found: {:?}", exe_to_launch));
        return ExitCode::FAILURE;
    }

    // Build arguments
    let args = config.process_start_args.clone();
    
    // Launch the process
    match launch_process(&exe_to_launch, &args, config.process_start_and_wait, &logger) {
        Ok(code) => {
            logger.info(&format!("Process exited with code: {}", code));
            if code == 0 {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            logger.error(&format!("Failed to launch process: {}", e));
            ExitCode::FAILURE
        }
    }
}
