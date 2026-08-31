//! Minimal native Windows tray host for the per-user beta runtime.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, MessageBoxW, PostMessageW,
    PostQuitMessage, RegisterClassW, SetForegroundWindow, TrackPopupMenu, TranslateMessage,
    CW_USEDEFAULT, MB_ICONERROR, MB_OK, MF_CHECKED, MF_DISABLED, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, MSG, SW_SHOWNORMAL, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CONTEXTMENU,
    WM_DESTROY, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};

use crate::diagnostics::{self, LogLevel};
use crate::startup::{RegistrationState, StartupRegistration};
use crate::transport::{AgentServiceConfig, ManagedService, ManagedServiceState};

const TRAY_ICON_ID: u32 = 1;
const TRAY_ICON_RESOURCE_ID: u16 = 2;
const TRAY_CALLBACK: u32 = WM_APP + 1;
const COMMAND_STARTUP: usize = 1001;
const COMMAND_EXIT: usize = 1002;
const COMMAND_LOG_INFO: usize = 1003;
const COMMAND_LOG_DEBUG: usize = 1004;
const COMMAND_OPEN_DIAGNOSTICS: usize = 1005;
const ENDPOINT: &str = "http://127.0.0.1:48480";

static APP: OnceLock<Mutex<TrayState>> = OnceLock::new();

pub fn show_fatal_error(message: &str) {
    unsafe { show_error(std::ptr::null_mut(), message) };
}

struct TrayState {
    service: Option<ManagedService>,
    startup_error: Option<String>,
    executable: PathBuf,
}

impl TrayState {
    fn service_label(&self) -> String {
        if self.startup_error.is_some() {
            return "Status: Startup failed (see diagnostics log)".to_string();
        }
        match self.service.as_ref().map(ManagedService::state) {
            Some(ManagedServiceState::Starting) => "Status: Starting".to_string(),
            Some(ManagedServiceState::Running) => "Status: Running".to_string(),
            Some(ManagedServiceState::Stopping) => "Status: Stopping".to_string(),
            Some(ManagedServiceState::Stopped) => "Status: Stopped".to_string(),
            Some(ManagedServiceState::Failed(_)) => {
                "Status: Failed (see diagnostics log)".to_string()
            }
            None => "Status: Stopped".to_string(),
        }
    }

    fn startup_state(&self) -> Result<RegistrationState, String> {
        StartupRegistration::current_user().state(&self.executable)
    }

    fn toggle_startup(&mut self) -> Result<&'static str, String> {
        let registration = StartupRegistration::current_user();
        match registration.state(&self.executable)? {
            RegistrationState::Enabled => {
                registration.disable()?;
                Ok("disabled")
            }
            RegistrationState::Missing | RegistrationState::Stale => {
                registration.enable(&self.executable)?;
                Ok("enabled")
            }
        }
    }

    fn stop_service(&mut self) -> Result<(), String> {
        match self.service.as_mut() {
            Some(service) => service.shutdown(),
            None => Ok(()),
        }
    }
}

pub fn run() -> Result<(), String> {
    diagnostics::set_lifecycle_state("tray_initializing");
    let executable = std::env::current_exe()
        .map_err(|error| format!("failed to locate the current executable: {error}"))?;
    diagnostics::info("tray_initialization_requested");
    let (service, startup_error) = match ManagedService::start(AgentServiceConfig::default()) {
        Ok(service) => {
            diagnostics::info("service_startup_succeeded listener=127.0.0.1:48480 scope=loopback");
            (Some(service), None)
        }
        Err(error) => {
            diagnostics::info(&format!("service_startup_failed error={error}"));
            (None, Some(error))
        }
    };
    APP.set(Mutex::new(TrayState {
        service,
        startup_error,
        executable,
    }))
    .map_err(|_| "tray runtime is already initialized".to_string())?;

    diagnostics::set_lifecycle_state("tray_running");
    diagnostics::info("tray_initialization_succeeded");
    let result = unsafe { run_message_loop() };
    if let Some(app) = APP.get() {
        let mut app = app.lock().unwrap_or_else(|error| error.into_inner());
        if let Err(error) = app.stop_service() {
            diagnostics::info(&format!("service_shutdown_failed error={error}"));
        }
    }
    result
}

unsafe fn run_message_loop() -> Result<(), String> {
    let instance = GetModuleHandleW(std::ptr::null());
    if instance.is_null() {
        return Err(last_error("resolve application module"));
    }
    let class_name = wide_null("ResonanceSignalTrayWindow");
    let window_class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: class_name.as_ptr(),
        ..Default::default()
    };
    if RegisterClassW(&window_class) == 0 {
        return Err(last_error("register tray window class"));
    }
    let title = wide_null("Resonance Signal");
    let window = CreateWindowExW(
        0,
        class_name.as_ptr(),
        title.as_ptr(),
        0,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        0,
        0,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        instance,
        std::ptr::null(),
    );
    if window.is_null() {
        return Err(last_error("create tray message window"));
    }
    add_tray_icon(window)?;

    let mut message = MSG::default();
    loop {
        let result = GetMessageW(&mut message, std::ptr::null_mut(), 0, 0);
        if result == -1 {
            remove_tray_icon(window);
            return Err(last_error("read tray message"));
        }
        if result == 0 {
            break;
        }
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        TRAY_CALLBACK
            if lparam as u32 == WM_RBUTTONUP
                || lparam as u32 == WM_CONTEXTMENU
                || lparam as u32 == WM_LBUTTONUP =>
        {
            show_menu(window);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(window, message, wparam, lparam),
    }
}

unsafe fn add_tray_icon(window: HWND) -> Result<(), String> {
    let instance = GetModuleHandleW(std::ptr::null());
    if instance.is_null() {
        return Err(last_error("resolve tray icon module"));
    }
    let icon_handle = LoadIconW(instance, resource_identifier(TRAY_ICON_RESOURCE_ID));
    if icon_handle.is_null() {
        return Err(last_error("load embedded tray icon"));
    }
    let mut icon = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: TRAY_ICON_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: TRAY_CALLBACK,
        hIcon: icon_handle,
        ..Default::default()
    };
    set_fixed_wide(&mut icon.szTip, "Resonance Signal");
    if Shell_NotifyIconW(NIM_ADD, &icon) == 0 {
        return Err(last_error("add tray icon"));
    }
    Ok(())
}

const fn resource_identifier(identifier: u16) -> *const u16 {
    identifier as usize as *const u16
}

unsafe fn update_tray_tooltip(window: HWND, text: &str) {
    let mut icon = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: TRAY_ICON_ID,
        uFlags: NIF_TIP,
        ..Default::default()
    };
    set_fixed_wide(&mut icon.szTip, text);
    let _ = Shell_NotifyIconW(NIM_MODIFY, &icon);
}

unsafe fn remove_tray_icon(window: HWND) {
    let icon = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: TRAY_ICON_ID,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &icon);
}

unsafe fn show_menu(window: HWND) {
    let Some(app) = APP.get() else { return };
    let mut app = app.lock().unwrap_or_else(|error| error.into_inner());
    let service_label = app.service_label();
    let startup_state = app.startup_state();
    let startup_enabled = matches!(startup_state, Ok(RegistrationState::Enabled));
    let startup_label = match &startup_state {
        Ok(RegistrationState::Stale) => "Start with Windows (stale entry)",
        Err(_) => "Start with Windows (unavailable)",
        _ => "Start with Windows",
    };
    update_tray_tooltip(
        window,
        &format!(
            "Resonance Signal - {}",
            service_label.trim_start_matches("Status: ")
        ),
    );

    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    append_text(
        menu,
        MF_STRING | MF_DISABLED | MF_GRAYED,
        0,
        "Resonance Signal",
    );
    append_text(menu, MF_STRING | MF_DISABLED | MF_GRAYED, 0, &service_label);
    append_text(menu, MF_STRING | MF_DISABLED | MF_GRAYED, 0, ENDPOINT);
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    append_text(menu, MF_STRING | MF_DISABLED | MF_GRAYED, 0, "Log Level");
    let log_level = diagnostics::current_level();
    let (info_flags, debug_flags) = log_level_flags(log_level);
    append_text(menu, info_flags, COMMAND_LOG_INFO, "Info");
    append_text(menu, debug_flags, COMMAND_LOG_DEBUG, "Debug");
    append_text(
        menu,
        MF_STRING,
        COMMAND_OPEN_DIAGNOSTICS,
        "Open Diagnostics Folder",
    );
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    let startup_flags = MF_STRING | if startup_enabled { MF_CHECKED } else { 0 };
    append_text(menu, startup_flags, COMMAND_STARTUP, startup_label);
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
    append_text(menu, MF_STRING, COMMAND_EXIT, "Exit");

    let mut point = POINT::default();
    if GetCursorPos(&mut point) != 0 {
        SetForegroundWindow(window);
        let command = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD,
            point.x,
            point.y,
            0,
            window,
            std::ptr::null(),
        ) as usize;
        let _ = PostMessageW(window, WM_NULL, 0, 0);
        match command {
            COMMAND_STARTUP => match app.toggle_startup() {
                Err(error) => {
                    diagnostics::info(&format!("startup_registration_change_failed error={error}"));
                    show_error(window, &error);
                }
                Ok(action) => diagnostics::info(&format!(
                    "startup_registration_changed action={action} command=direct_executable_tray"
                )),
            },
            COMMAND_LOG_INFO => set_log_level(window, LogLevel::Info),
            COMMAND_LOG_DEBUG => set_log_level(window, LogLevel::Debug),
            COMMAND_OPEN_DIAGNOSTICS => {
                if let Err(error) = open_diagnostics_folder() {
                    diagnostics::info(&format!("diagnostics_folder_open_failed error={error}"));
                    show_error(window, &error);
                } else {
                    diagnostics::info("diagnostics_folder_opened mechanism=windows_shell");
                }
            }
            COMMAND_EXIT => {
                diagnostics::set_lifecycle_state("tray_exit_requested");
                diagnostics::info("tray_exit_requested");
                remove_tray_icon(window);
                if let Err(error) = app.stop_service() {
                    diagnostics::info(&format!("service_shutdown_failed error={error}"));
                    show_error(window, &error);
                } else {
                    diagnostics::info("service_shutdown_succeeded reason=tray_exit");
                }
                DestroyWindow(window);
            }
            _ => {}
        }
    }
    DestroyMenu(menu);
}

fn log_level_flags(level: LogLevel) -> (u32, u32) {
    match level {
        LogLevel::Info => (MF_STRING | MF_CHECKED, MF_STRING),
        LogLevel::Debug => (MF_STRING, MF_STRING | MF_CHECKED),
    }
}

unsafe fn set_log_level(window: HWND, level: LogLevel) {
    if let Err(error) = diagnostics::set_level(level) {
        diagnostics::info(&format!("log_level_change_failed error={error}"));
        show_error(window, &error);
    }
}

unsafe fn open_diagnostics_folder() -> Result<(), String> {
    let directory = diagnostics::directory()?;
    let directory = wide_null_os(directory.as_os_str());
    let result = ShellExecuteW(
        std::ptr::null_mut(),
        std::ptr::null(),
        directory.as_ptr(),
        std::ptr::null(),
        std::ptr::null(),
        SW_SHOWNORMAL,
    );
    if result as isize <= 32 {
        Err(format!(
            "failed to open diagnostics directory: Windows shell result {}",
            result as isize
        ))
    } else {
        Ok(())
    }
}

unsafe fn append_text(menu: *mut core::ffi::c_void, flags: u32, id: usize, text: &str) {
    let text = wide_null(text);
    let _ = AppendMenuW(menu, flags, id, text.as_ptr());
}

unsafe fn show_error(window: HWND, message: &str) {
    let title = wide_null("Resonance Signal");
    let message = wide_null(message);
    MessageBoxW(
        window,
        message.as_ptr(),
        title.as_ptr(),
        MB_OK | MB_ICONERROR,
    );
}

fn set_fixed_wide<const N: usize>(target: &mut [u16; N], value: &str) {
    let encoded = OsStr::new(value).encode_wide().take(N.saturating_sub(1));
    for (slot, unit) in target.iter_mut().zip(encoded) {
        *slot = unit;
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    wide_null_os(OsStr::new(value))
}

fn wide_null_os(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn last_error(operation: &str) -> String {
    format!("failed to {operation}: {}", std::io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_wide_text_is_bounded_and_null_terminated() {
        let mut target = [0_u16; 5];
        set_fixed_wide(&mut target, "abcdef");
        assert_eq!(&target, &[97, 98, 99, 100, 0]);
    }

    #[test]
    fn tray_icon_uses_the_embedded_resource_identifier() {
        assert_eq!(resource_identifier(TRAY_ICON_RESOURCE_ID) as usize, 2);
    }

    #[test]
    fn tray_menu_checks_exactly_the_current_log_level() {
        assert_eq!(
            log_level_flags(LogLevel::Info),
            (MF_STRING | MF_CHECKED, MF_STRING)
        );
        assert_eq!(
            log_level_flags(LogLevel::Debug),
            (MF_STRING, MF_STRING | MF_CHECKED)
        );
    }
}
