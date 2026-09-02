// NetPrintSwitch
// Copyright (C) 2026 fattminer
// SPDX-License-Identifier: LicenseRef-NetPrintSwitch-AGPL-3.0-only-PLUS-Commons-Clause-1.0

#![cfg_attr(windows, windows_subsystem = "windows")]

mod winui;

use serde::{Deserialize, Serialize};
use std::{
    env,
    ffi::OsStr,
    fs,
    os::windows::ffi::OsStrExt,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{self, Child, Command, Output, Stdio},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};
use windows::{
    core::{BOOL, PCWSTR},
    Win32::{
        Foundation::{
            CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT,
            POINT, WAIT_ABANDONED, WAIT_OBJECT_0, WPARAM,
        },
        Graphics::Gdi::{
            CreateFontW, GetSysColorBrush, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_WINDOW,
            DEFAULT_CHARSET, DEFAULT_PITCH, FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD, HFONT,
            OUT_DEFAULT_PRECIS,
        },
        Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH},
        System::{
            LibraryLoader::GetModuleHandleW,
            Threading::{
                CreateMutexW, ReleaseMutex, WaitForSingleObject, CREATE_NO_WINDOW, INFINITE,
            },
        },
        UI::{
            Shell::{
                Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD,
                NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
                DestroyMenu, DestroyWindow, DispatchMessageW, EnumWindows, FindWindowW,
                GetCursorPos, GetMessageW, GetWindowTextLengthW, GetWindowTextW,
                GetWindowThreadProcessId, LoadCursorW, LoadIconW, LoadImageW, PostMessageW,
                PostQuitMessage, RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowTextW,
                ShowWindow, TrackPopupMenu, TranslateMessage, BS_DEFPUSHBUTTON, BS_PUSHBUTTON,
                CBS_DROPDOWNLIST, CB_ADDSTRING, CB_GETCURSEL, CB_RESETCONTENT, CW_USEDEFAULT,
                ES_AUTOHSCROLL, ES_LEFT, HICON, HMENU, IDC_ARROW, IDI_APPLICATION, IMAGE_ICON,
                LBS_NOTIFY, LB_ADDSTRING, LB_GETCURSEL, LB_RESETCONTENT, LR_DEFAULTSIZE,
                LR_LOADFROMFILE, MB_ICONERROR, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_TOPMOST,
                MB_YESNO, MF_SEPARATOR, MF_STRING, MSG, SW_HIDE, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE,
                WM_COMMAND, WM_CREATE, WM_DESTROY, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WM_SETFONT,
                WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_CLIENTEDGE, WS_EX_TOOLWINDOW,
                WS_OVERLAPPEDWINDOW, WS_VISIBLE,
            },
        },
    },
};

const WM_TRAY: u32 = WM_APP + 1;
const WM_CHECK_NETWORK: u32 = WM_APP + 2;
const WM_EXIT_APP: u32 = WM_APP + 3;
const WM_RETRY_NETWORK: u32 = WM_APP + 4;
const WM_NETWORK_RESULT: u32 = WM_APP + 5;
const WM_UI_SHOW: u32 = WM_APP + 6;
const WM_UI_EXIT: u32 = WM_APP + 7;
const WM_UI_REFRESH: u32 = WM_APP + 8;
const WM_UI_SHOW_RETRY: u32 = WM_APP + 9;
const WM_UI_HOOK_FAILED: u32 = WM_APP + 10;
const ID_SHOW: usize = 2001;
const ID_REFRESH: usize = 2002;
const ID_EXIT: usize = 2003;
const ID_SAVE: usize = 3001;
const ID_DELETE: usize = 3002;
const ID_PROMPT_MODE: usize = 3003;
const ID_NETWORK: usize = 3004;
const ID_PRINTERS: usize = 3005;
const ID_ASSOCIATIONS: usize = 3006;
const TASK_NAME: &str = "NetPrintSwitch\\NetworkConnected";
const NETWORK_LOG: &str = "Microsoft-Windows-NetworkProfile/Operational";

static STATE: OnceLock<Mutex<AppState>> = OnceLock::new();
static RETRY_SCHEDULED: OnceLock<Mutex<bool>> = OnceLock::new();
static EXIT_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CHECK_IN_PROGRESS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CHECK_PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PROMPT_IN_PROGRESS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static RETRY_ATTEMPT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static RETRY_ERROR_NOTIFIED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
type PrinterCache = Mutex<Option<(Instant, Vec<Printer>)>>;
static PRINTER_CACHE: OnceLock<PrinterCache> = OnceLock::new();

struct NetworkCheckResult {
    connection_event: bool,
    network: Result<Option<Network>, String>,
    printers: Option<Result<Vec<Printer>, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Association {
    pub(crate) network: String,
    pub(crate) network_type: String,
    pub(crate) printer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PromptMode {
    #[default]
    OncePerConnection,
    EveryConnection,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CloseBehavior {
    #[default]
    AskOncePerStartup,
    AlwaysMinimize,
    AlwaysQuit,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct Config {
    pub(crate) associations: Vec<Association>,
    #[serde(default)]
    pub(crate) prompt_mode: PromptMode,
    #[serde(default)]
    pub(crate) close_behavior: CloseBehavior,
}

#[derive(Debug, Clone)]
pub(crate) struct Network {
    pub(crate) name: String,
    pub(crate) network_type: String,
}

impl Network {
    fn key(&self) -> String {
        format!("{}::{}", self.network_type, normalize(&self.name))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Printer {
    pub(crate) name: String,
    pub(crate) is_default: bool,
}

enum CheckPrompt {
    Duplicate {
        network: Network,
        printers: Vec<String>,
    },
    Switch {
        network: Network,
        printer: String,
    },
}

struct AppState {
    config: Config,
    body_font: HFONT,
    title_font: HFONT,
    title_label: HWND,
    subtitle_label: HWND,
    network_label: HWND,
    network_edit: HWND,
    printer_combo: HWND,
    prompt_combo: HWND,
    associations_list: HWND,
    printers: Vec<Printer>,
    printers_loaded: bool,
    current_network: Option<Network>,
    last_network_key: Option<String>,
    last_prompted_network_key: Option<String>,
    last_prompted_association_count: Option<usize>,
    last_connection_event: Option<(String, Instant)>,
    tray_added: bool,
    tray_icon: HICON,
    tray_icon_owned: bool,
}

// All HWND access occurs on the GUI thread. Mutex only protects initialization and callbacks.
unsafe impl Send for AppState {}

impl AppState {
    fn new(config: Config) -> Self {
        Self {
            config,
            body_font: HFONT::default(),
            title_font: HFONT::default(),
            title_label: HWND::default(),
            subtitle_label: HWND::default(),
            network_label: HWND::default(),
            network_edit: HWND::default(),
            printer_combo: HWND::default(),
            prompt_combo: HWND::default(),
            associations_list: HWND::default(),
            printers: Vec::new(),
            printers_loaded: false,
            current_network: None,
            last_network_key: None,
            last_prompted_network_key: None,
            last_prompted_association_count: None,
            last_connection_event: None,
            tray_added: false,
            tray_icon: HICON::default(),
            tray_icon_owned: false,
        }
    }
}

fn main() -> windows::core::Result<()> {
    if env::args().any(|arg| arg == "--ui") {
        let ui_mutex_name = wide("Local\\NetPrintSwitch.UI");
        let _ui_mutex = unsafe { CreateMutexW(None, true, PCWSTR(ui_mutex_name.as_ptr()))? };
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        return winui::run().map_err(|error| {
            windows::core::Error::new(windows::core::HRESULT(0x80004005u32 as i32), error)
        });
    }
    let check_only = env::args().any(|arg| arg == "--check-network");
    let instance_name = wide("Local\\NetPrintSwitch");
    let _instance_mutex = unsafe { CreateMutexW(None, true, PCWSTR(instance_name.as_ptr()))? };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        if let Some(existing) = find_main_window_with_retry() {
            unsafe {
                if check_only {
                    let _ = PostMessageW(Some(existing), WM_CHECK_NETWORK, WPARAM(0), LPARAM(0));
                } else {
                    if let Err(error) = spawn_ui() {
                        let _ = message(
                            existing,
                            &format!("NetPrintSwitch could not open its WinUI window.\n\n{error}"),
                            "User interface unavailable",
                            MB_OK | MB_ICONERROR | MB_TOPMOST,
                        );
                    }
                }
            }
        }
        return Ok(());
    }
    let (config, config_error) = match load_config_result() {
        Ok(config) => (config, None),
        Err(error) => (Config::default(), Some(error)),
    };
    let _ = STATE.set(Mutex::new(AppState::new(config)));
    let instance = unsafe { GetModuleHandleW(None)? };
    let class_name = wide("NetPrintSwitchWindow");
    let window_title = wide("NetPrintSwitch");
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW)? };
    let class = WNDCLASSW {
        hCursor: cursor,
        hInstance: HINSTANCE(instance.0),
        lpszClassName: PCWSTR(class_name.as_ptr()),
        lpfnWndProc: Some(window_proc),
        hbrBackground: unsafe { GetSysColorBrush(COLOR_WINDOW) },
        ..Default::default()
    };
    unsafe { RegisterClassW(&class) };

    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(window_title.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            700,
            650,
            None,
            None,
            Some(HINSTANCE(instance.0)),
            None,
        )?
    };
    if let Some(error) = config_error {
        unsafe {
            let _ = message(
                hwnd,
                &format!(
                    "NetPrintSwitch could not load its configuration. Associations were not loaded.\n\n{error}\n\nFix the configuration backup, then restart NetPrintSwitch."
                ),
                "Configuration unavailable",
                MB_OK | MB_ICONERROR | MB_TOPMOST,
            );
        }
    }
    if !check_only {
        if let Err(error) = spawn_ui() {
            unsafe {
                let _ = crate::message(
                    hwnd,
                    &format!("NetPrintSwitch could not open its WinUI window.\n\n{error}"),
                    "User interface unavailable",
                    MB_OK | MB_ICONERROR | MB_TOPMOST,
                );
            }
        }
    }
    if let Err(error) = install_scheduled_task() {
        unsafe {
            balloon(hwnd, "Network trigger unavailable", &error);
            let details = format!(
                concat!(
                    "NetPrintSwitch could not enable automatic network monitoring.\n\n",
                    "Task Scheduler error:\n{error}\n\n",
                    "Common causes:\n",
                    "• Windows Task Scheduler is disabled or unavailable.\n",
                    "• Your account or organization blocks user-created scheduled tasks.\n",
                    "• The executable path is inaccessible or no longer exists.\n",
                    "• Security software blocked task registration.\n",
                    "• The NetworkProfile event log is disabled.\n\n",
                    "How to fix:\n",
                    "1. Confirm Task Scheduler is running.\n",
                    "2. Check that you can create tasks for your Windows account.\n",
                    "3. Restart NetPrintSwitch after correcting the problem.\n",
                    "4. If needed, remove a stale task named NetPrintSwitch\\NetworkConnected and restart the app.\n\n",
                    "Automatic switching is unavailable until this is fixed."
                ),
                error = error,
            );
            let _ = message(
                hwnd,
                &details,
                "Automatic monitoring unavailable",
                MB_OK | MB_ICONERROR | MB_TOPMOST,
            );
        }
    }

    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.into() {
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_CREATE => {
            create_controls(hwnd);
            if let Err(error) = add_tray_icon(hwnd) {
                let _ = crate::message(
                    hwnd,
                    &format!(
                        "NetPrintSwitch could not add its system-tray icon. Automatic monitoring may still run, but tray controls will be unavailable.\n\n{error}"
                    ),
                    "System-tray unavailable",
                    MB_OK | MB_ICONERROR | MB_TOPMOST,
                );
            }
            refresh_ui(hwnd);
            start_network_check(hwnd, true, true);
            LRESULT(0)
        }
        WM_CHECK_NETWORK => {
            start_network_check(hwnd, true, true);
            LRESULT(0)
        }
        WM_RETRY_NETWORK => {
            start_network_check(hwnd, true, true);
            LRESULT(0)
        }
        WM_UI_SHOW_RETRY => {
            show_window_retry(hwnd, wparam.0 as u32);
            LRESULT(0)
        }
        WM_UI_HOOK_FAILED => {
            let _ = crate::message(
                hwnd,
                "NetPrintSwitch could not enable its close-window handler. The WinUI window may close without showing the minimize-or-quit prompt. Restart NetPrintSwitch to try again.",
                "Close behavior unavailable",
                MB_OK | MB_ICONWARNING | MB_TOPMOST,
            );
            LRESULT(0)
        }
        WM_NETWORK_RESULT => {
            let result = *Box::from_raw(lparam.0 as *mut NetworkCheckResult);
            handle_network_result(hwnd, result);
            LRESULT(0)
        }
        WM_COMMAND => {
            handle_command(hwnd, wparam.0);
            LRESULT(0)
        }
        WM_TRAY => {
            let event = lparam.0 as u32;
            if event == WM_LBUTTONUP {
                show_window(hwnd);
            } else if event == WM_RBUTTONUP {
                refresh_ui_window();
                show_tray_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_EXIT_APP => {
            EXIT_REQUESTED.store(true, std::sync::atomic::Ordering::Release);
            request_ui_exit();
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            if EXIT_REQUESTED.load(std::sync::atomic::Ordering::Acquire) {
                if let Err(error) = remove_scheduled_task() {
                    let _ = crate::message(
                        hwnd,
                        &format!(
                            "NetPrintSwitch could not remove its scheduled network task.\n\n{error}\n\nAutomatic monitoring may start NetPrintSwitch again. Remove task '{TASK_NAME}' in Task Scheduler if needed."
                        ),
                        "Could not remove scheduled task",
                        MB_OK | MB_ICONERROR | MB_TOPMOST,
                    );
                }
            }
            let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd, false));
            let (body_font, title_font, tray_icon, tray_icon_owned) = {
                let mut state = STATE.get().unwrap().lock().unwrap();
                let resources = (
                    state.body_font,
                    state.title_font,
                    state.tray_icon,
                    state.tray_icon_owned,
                );
                state.body_font = HFONT::default();
                state.title_font = HFONT::default();
                state.tray_icon = HICON::default();
                state.tray_icon_owned = false;
                resources
            };
            if !body_font.is_invalid() {
                let _ = windows::Win32::Graphics::Gdi::DeleteObject(
                    windows::Win32::Graphics::Gdi::HGDIOBJ(body_font.0),
                );
            }
            if !title_font.is_invalid() {
                let _ = windows::Win32::Graphics::Gdi::DeleteObject(
                    windows::Win32::Graphics::Gdi::HGDIOBJ(title_font.0),
                );
            }
            if tray_icon_owned && !tray_icon.is_invalid() {
                let _ = DestroyIcon(tray_icon);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn create_controls(hwnd: HWND) {
    let body_face = wide("Segoe UI");
    let title_face = wide("Segoe UI");
    let body_font = CreateFontW(
        -16,
        0,
        0,
        0,
        FW_NORMAL.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
        PCWSTR(body_face.as_ptr()),
    );
    let title_font = CreateFontW(
        -28,
        0,
        0,
        0,
        FW_SEMIBOLD.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
        PCWSTR(title_face.as_ptr()),
    );
    {
        let mut state = STATE.get().unwrap().lock().unwrap();
        state.body_font = body_font;
        state.title_font = title_font;
    }

    let title_label = label(hwnd, "NetPrintSwitch", 28, 22, 620, 38, 1100);
    let subtitle_label = label(
        hwnd,
        "Switch default printer by network.",
        30,
        61,
        620,
        24,
        1101,
    );
    label(hwnd, "CURRENT NETWORK", 30, 108, 220, 18, 0);
    let network_label = label(hwnd, "Detecting...", 30, 128, 620, 30, 1001);
    label(hwnd, "CREATE ASSOCIATION", 30, 182, 300, 22, 0);
    label(hwnd, "Network name / SSID", 30, 222, 175, 22, 0);
    let network_edit = edit(hwnd, 220, 219, 425, 28, ID_NETWORK);
    label(hwnd, "Installed printer", 30, 263, 175, 22, 0);
    let printer_combo = combo(hwnd, 220, 260, 425, 250, ID_PRINTERS);
    label(hwnd, "Prompt behavior", 30, 304, 175, 22, 0);
    let prompt_combo = combo(hwnd, 220, 301, 425, 110, ID_PROMPT_MODE);
    let once = wide("Prompt once per connection");
    let every = wide("Prompt every connection");
    send_message(
        prompt_combo,
        CB_ADDSTRING,
        WPARAM(0),
        LPARAM(once.as_ptr() as isize),
    );
    send_message(
        prompt_combo,
        CB_ADDSTRING,
        WPARAM(0),
        LPARAM(every.as_ptr() as isize),
    );
    let mode_index = match STATE.get().unwrap().lock().unwrap().config.prompt_mode {
        PromptMode::OncePerConnection => 0,
        PromptMode::EveryConnection => 1,
    };
    send_message(
        prompt_combo,
        windows::Win32::UI::WindowsAndMessaging::CB_SETCURSEL,
        WPARAM(mode_index),
        LPARAM(0),
    );
    primary_button(hwnd, "Save association", 220, 340, 185, 34, ID_SAVE);
    button(hwnd, "Refresh", 420, 340, 105, 34, ID_REFRESH);
    label(hwnd, "SAVED ASSOCIATIONS", 30, 414, 300, 22, 0);
    let associations_list = listbox(hwnd, 30, 444, 615, 120, ID_ASSOCIATIONS);
    button(hwnd, "Delete selected", 30, 580, 145, 34, ID_DELETE);
    label(
        hwnd,
        "Runs quietly in the tray. Close this window to keep monitoring.",
        220,
        588,
        425,
        22,
        0,
    );
    let mut state = STATE.get().unwrap().lock().unwrap();
    state.title_label = title_label;
    state.subtitle_label = subtitle_label;
    state.network_label = network_label;
    state.network_edit = network_edit;
    state.printer_combo = printer_combo;
    state.prompt_combo = prompt_combo;
    state.associations_list = associations_list;
    let title_font = state.title_font;
    drop(state);
    let _ = send_message(
        title_label,
        WM_SETFONT,
        WPARAM(title_font.0 as usize),
        LPARAM(1),
    );
}

unsafe fn label(
    parent: HWND,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: usize,
) -> HWND {
    control(
        parent,
        ControlSpec {
            class: "STATIC",
            text,
            x,
            y,
            width,
            height,
            id,
            ex_style: Default::default(),
            style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(0),
        },
    )
}

unsafe fn edit(parent: HWND, x: i32, y: i32, width: i32, height: i32, id: usize) -> HWND {
    control(
        parent,
        ControlSpec {
            class: "EDIT",
            text: "",
            x,
            y,
            width,
            height,
            id,
            ex_style: WS_EX_CLIENTEDGE,
            style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
                WS_CHILD.0 | WS_VISIBLE.0 | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
            ),
        },
    )
}

unsafe fn combo(parent: HWND, x: i32, y: i32, width: i32, height: i32, id: usize) -> HWND {
    control(
        parent,
        ControlSpec {
            class: "COMBOBOX",
            text: "",
            x,
            y,
            width,
            height,
            id,
            ex_style: Default::default(),
            style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
                WS_CHILD.0 | WS_VISIBLE.0 | CBS_DROPDOWNLIST as u32,
            ),
        },
    )
}

unsafe fn listbox(parent: HWND, x: i32, y: i32, width: i32, height: i32, id: usize) -> HWND {
    control(
        parent,
        ControlSpec {
            class: "LISTBOX",
            text: "",
            x,
            y,
            width,
            height,
            id,
            ex_style: Default::default(),
            style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
                WS_CHILD.0 | WS_VISIBLE.0 | LBS_NOTIFY as u32,
            ),
        },
    )
}

unsafe fn button(
    parent: HWND,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: usize,
) -> HWND {
    control(
        parent,
        ControlSpec {
            class: "BUTTON",
            text,
            x,
            y,
            width,
            height,
            id,
            ex_style: Default::default(),
            style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
                WS_CHILD.0 | WS_VISIBLE.0 | BS_PUSHBUTTON as u32,
            ),
        },
    )
}

unsafe fn primary_button(
    parent: HWND,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: usize,
) -> HWND {
    control(
        parent,
        ControlSpec {
            class: "BUTTON",
            text,
            x,
            y,
            width,
            height,
            id,
            ex_style: Default::default(),
            style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(
                WS_CHILD.0 | WS_VISIBLE.0 | BS_DEFPUSHBUTTON as u32,
            ),
        },
    )
}

struct ControlSpec<'a> {
    class: &'a str,
    text: &'a str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: usize,
    ex_style: windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE,
    style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE,
}

unsafe fn control(parent: HWND, spec: ControlSpec<'_>) -> HWND {
    let class = wide(spec.class);
    let text = wide(spec.text);
    let control_hwnd = CreateWindowExW(
        spec.ex_style,
        PCWSTR(class.as_ptr()),
        PCWSTR(text.as_ptr()),
        spec.style,
        spec.x,
        spec.y,
        spec.width,
        spec.height,
        Some(parent),
        Some(HMENU(spec.id as *mut std::ffi::c_void)),
        None,
        None,
    )
    .unwrap_or_default();
    if let Some(state) = STATE.get() {
        let state = state.lock().unwrap();
        let _ = send_message(
            control_hwnd,
            WM_SETFONT,
            WPARAM(state.body_font.0 as usize),
            LPARAM(1),
        );
    }
    control_hwnd
}

unsafe fn send_message(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    SendMessageW(hwnd, message, Some(wparam), Some(lparam))
}

unsafe fn handle_command(hwnd: HWND, command: usize) {
    let id = command & 0xffff;
    match id {
        ID_SHOW => show_window(hwnd),
        ID_REFRESH => {
            refresh_ui(hwnd);
            refresh_ui_window();
            start_network_check(hwnd, true, false);
        }
        ID_EXIT => {
            EXIT_REQUESTED.store(true, std::sync::atomic::Ordering::Release);
            request_ui_exit();
            let _ = DestroyWindow(hwnd);
        }
        ID_SAVE => save_association(hwnd),
        ID_DELETE => delete_association(hwnd),
        ID_PROMPT_MODE => save_prompt_mode(hwnd),
        _ => {}
    }
}

unsafe fn show_window(hwnd: HWND) {
    if let Some(ui_hwnd) = find_ui_window() {
        let _ = PostMessageW(Some(ui_hwnd), WM_UI_SHOW, WPARAM(0), LPARAM(0));
        let _ = PostMessageW(Some(ui_hwnd), WM_UI_REFRESH, WPARAM(0), LPARAM(0));
        return;
    }
    if let Err(error) = spawn_ui() {
        let _ = message(
            hwnd,
            &format!("NetPrintSwitch could not open its WinUI window.\n\n{error}"),
            "User interface unavailable",
            MB_OK | MB_ICONERROR | MB_TOPMOST,
        );
        return;
    }
    schedule_ui_show_retry(hwnd, 0);
}

fn find_ui_window() -> Option<HWND> {
    let mut found = HWND::default();
    unsafe {
        let _ = EnumWindows(
            Some(find_ui_window_callback),
            LPARAM(&mut found as *mut HWND as isize),
        );
    }
    (!found.0.is_null()).then_some(found)
}

unsafe extern "system" fn find_ui_window_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let mut process_id = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    if process_id == process::id() {
        return BOOL(1);
    }
    let length = GetWindowTextLengthW(hwnd);
    if length <= 0 {
        return BOOL(1);
    }
    let mut title = vec![0u16; length as usize + 1];
    let read = GetWindowTextW(hwnd, &mut title);
    if String::from_utf16_lossy(&title[..read as usize]) == "NetPrintSwitch" {
        *(lparam.0 as *mut HWND) = hwnd;
        return BOOL(0);
    }
    BOOL(1)
}

unsafe fn show_window_retry(hwnd: HWND, attempt: u32) {
    if let Some(ui_hwnd) = find_ui_window() {
        let _ = PostMessageW(Some(ui_hwnd), WM_UI_SHOW, WPARAM(0), LPARAM(0));
        let _ = PostMessageW(Some(ui_hwnd), WM_UI_REFRESH, WPARAM(0), LPARAM(0));
    } else if attempt < 40 {
        schedule_ui_show_retry(hwnd, attempt + 1);
    }
}

fn schedule_ui_show_retry(hwnd: HWND, attempt: u32) {
    let hwnd = hwnd.0 as usize;
    let _ = thread::Builder::new()
        .name("ui-show-retry".to_string())
        .spawn(move || {
            thread::sleep(Duration::from_millis(25));
            unsafe {
                let hwnd = HWND(hwnd as *mut std::ffi::c_void);
                let _ = PostMessageW(
                    Some(hwnd),
                    WM_UI_SHOW_RETRY,
                    WPARAM(attempt as usize),
                    LPARAM(0),
                );
            }
        });
}

fn request_ui_exit() {
    for _ in 0..120 {
        if let Some(ui_hwnd) = find_ui_window() {
            unsafe {
                let _ = PostMessageW(Some(ui_hwnd), WM_UI_EXIT, WPARAM(0), LPARAM(0));
            }
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn spawn_ui() -> Result<(), String> {
    let exe = env::current_exe().map_err(|error| error.to_string())?;
    Command::new(exe)
        .arg("--ui")
        .creation_flags(CREATE_NO_WINDOW.0)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

unsafe fn refresh_ui(hwnd: HWND) {
    let config = match load_config_result() {
        Ok(config) => Some(config),
        Err(error) => {
            report_check_error(hwnd, &error);
            None
        }
    };
    let (
        printer_combo,
        associations_list,
        network_label,
        network_edit,
        printers,
        associations,
        network,
    ) = {
        let mut state = STATE.get().unwrap().lock().unwrap();
        if let Some(config) = config {
            state.config = config;
        }
        (
            state.printer_combo,
            state.associations_list,
            state.network_label,
            state.network_edit,
            state.printers.clone(),
            state.config.associations.clone(),
            state.current_network.clone(),
        )
    };
    send_message(printer_combo, CB_RESETCONTENT, WPARAM(0), LPARAM(0));
    for printer in &printers {
        let display = if printer.is_default {
            format!("{} (default)", printer.name)
        } else {
            printer.name.clone()
        };
        let display = wide(&display);
        send_message(
            printer_combo,
            CB_ADDSTRING,
            WPARAM(0),
            LPARAM(display.as_ptr() as isize),
        );
    }
    if !printers.is_empty() {
        let selected = printers
            .iter()
            .position(|printer| printer.is_default)
            .unwrap_or(0);
        send_message(
            printer_combo,
            windows::Win32::UI::WindowsAndMessaging::CB_SETCURSEL,
            WPARAM(selected),
            LPARAM(0),
        );
    }
    send_message(associations_list, LB_RESETCONTENT, WPARAM(0), LPARAM(0));
    for association in &associations {
        let display = wide(&format!(
            "{} [{}]  =>  {}",
            association.network, association.network_type, association.printer
        ));
        send_message(
            associations_list,
            LB_ADDSTRING,
            WPARAM(0),
            LPARAM(display.as_ptr() as isize),
        );
    }
    let network_text = network
        .as_ref()
        .map(|n| format!("{} ({})", n.name, n.network_type))
        .unwrap_or_else(|| "No active network".to_string());
    let network_text_w = wide(&network_text);
    let _ = SetWindowTextW(network_label, PCWSTR(network_text_w.as_ptr()));
    let network_w = wide(
        network
            .as_ref()
            .map(|network| network.name.as_str())
            .unwrap_or_default(),
    );
    let _ = SetWindowTextW(network_edit, PCWSTR(network_w.as_ptr()));
}

fn refresh_ui_window() {
    if let Some(ui_hwnd) = find_ui_window() {
        unsafe {
            let _ = PostMessageW(Some(ui_hwnd), WM_UI_REFRESH, WPARAM(0), LPARAM(0));
        }
    }
}

unsafe fn save_association(hwnd: HWND) {
    let state = STATE.get().unwrap().lock().unwrap();
    let network = get_text(state.network_edit);
    let selected = send_message(state.printer_combo, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as usize;
    if network.trim().is_empty() || selected >= state.printers.len() {
        message(
            hwnd,
            "Choose a network and installed printer first.",
            "NetPrintSwitch",
            MB_OK | MB_ICONERROR,
        );
        return;
    }
    let printer = state.printers[selected].name.clone();
    let network_type = state
        .current_network
        .as_ref()
        .filter(|n| normalize(&n.name) == normalize(&network))
        .map(|n| n.network_type.clone())
        .unwrap_or_else(|| "Network".to_string());
    let association = Association {
        network: network.trim().to_string(),
        network_type,
        printer,
    };
    let mut config = state.config.clone();
    config.associations.retain(|existing| {
        !(normalize(&existing.network) == normalize(&association.network)
            && (existing
                .network_type
                .eq_ignore_ascii_case(&association.network_type)
                || existing.network_type.eq_ignore_ascii_case("Network")
                || association.network_type.eq_ignore_ascii_case("Network")))
    });
    config.associations.push(association);
    drop(state);
    if let Err(error) = save_config(&config) {
        message(
            hwnd,
            &format!("Could not save configuration: {error}"),
            "NetPrintSwitch",
            MB_OK | MB_ICONERROR,
        );
    } else {
        STATE.get().unwrap().lock().unwrap().config = config;
        refresh_ui(hwnd);
        start_network_check(hwnd, false, false);
        balloon(
            hwnd,
            "Association saved",
            "NetPrintSwitch will suggest this printer on this network.",
        );
    }
}

unsafe fn delete_association(hwnd: HWND) {
    let state = STATE.get().unwrap().lock().unwrap();
    let selected =
        send_message(state.associations_list, LB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as usize;
    if selected >= state.config.associations.len() {
        return;
    }
    let name = state.config.associations[selected].network.clone();
    let answer = message(
        hwnd,
        &format!("Delete association for {name}?"),
        "NetPrintSwitch",
        MB_YESNO | MB_ICONQUESTION,
    );
    if answer != windows::Win32::UI::WindowsAndMessaging::IDYES {
        return;
    }
    let mut config = state.config.clone();
    config.associations.remove(selected);
    drop(state);
    match save_config(&config) {
        Ok(()) => {
            STATE.get().unwrap().lock().unwrap().config = config;
            refresh_ui(hwnd);
            refresh_ui_window();
            start_network_check(hwnd, false, false);
        }
        Err(error) => {
            message(
                hwnd,
                &format!("Could not save changes: {error}"),
                "NetPrintSwitch",
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

unsafe fn save_prompt_mode(hwnd: HWND) {
    let state = STATE.get().unwrap().lock().unwrap();
    let selected = send_message(state.prompt_combo, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0;
    let mut config = state.config.clone();
    config.prompt_mode = if selected == 1 {
        PromptMode::EveryConnection
    } else {
        PromptMode::OncePerConnection
    };
    drop(state);
    match save_config(&config) {
        Ok(()) => STATE.get().unwrap().lock().unwrap().config = config,
        Err(error) => {
            message(
                hwnd,
                &format!("Could not save prompt preference: {error}"),
                "NetPrintSwitch",
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

unsafe fn clear_network_state(_hwnd: HWND) {
    let mut state = STATE.get().unwrap().lock().unwrap();
    state.current_network = None;
    state.last_network_key = None;
    state.last_prompted_network_key = None;
    state.last_prompted_association_count = None;
    state.last_connection_event = None;
    let _ = SetWindowTextW(state.network_edit, PCWSTR(wide("").as_ptr()));
    let _ = SetWindowTextW(
        state.network_label,
        PCWSTR(wide("No active network").as_ptr()),
    );
}

unsafe fn evaluate_network(hwnd: HWND, connection_event: bool, network: Network) {
    let key = network.key();
    let mut prompt: Option<CheckPrompt> = None;
    {
        let mut state = STATE.get().unwrap().lock().unwrap();
        if !state.printers_loaded {
            drop(state);
            schedule_network_retry(hwnd);
            return;
        }
        let config = match load_config_result() {
            Ok(config) => config,
            Err(error) => {
                drop(state);
                report_check_error(hwnd, &error);
                schedule_network_retry(hwnd);
                return;
            }
        };
        state.config = config;
        let changed = state.last_network_key.as_deref() != Some(&key);
        state.current_network = Some(network.clone());
        state.last_network_key = Some(key.clone());
        let event_is_duplicate = connection_event
            && state
                .last_connection_event
                .as_ref()
                .is_some_and(|(last_key, at)| {
                    last_key == &key && at.elapsed() < Duration::from_secs(5)
                });
        if connection_event && !event_is_duplicate {
            state.last_connection_event = Some((key.clone(), Instant::now()));
        }
        if changed || connection_event {
            let network_text = wide(&format!("{} ({})", network.name, network.network_type));
            let _ = SetWindowTextW(state.network_label, PCWSTR(network_text.as_ptr()));
            let edit_text = wide(&network.name);
            let _ = SetWindowTextW(state.network_edit, PCWSTR(edit_text.as_ptr()));
        }
        let associated: Vec<Association> = state
            .config
            .associations
            .iter()
            .filter(|association| association_matches(association, &network))
            .cloned()
            .collect();
        let should_evaluate = match state.config.prompt_mode {
            PromptMode::OncePerConnection => {
                changed
                    || state.last_prompted_network_key.as_deref() != Some(&key)
                    || state.last_prompted_association_count != Some(associated.len())
            }
            PromptMode::EveryConnection => connection_event && !event_is_duplicate,
        };
        if should_evaluate {
            if associated.len() > 1 {
                state.last_prompted_network_key = Some(key.clone());
                state.last_prompted_association_count = Some(associated.len());
                prompt = Some(CheckPrompt::Duplicate {
                    network: network.clone(),
                    printers: associated.into_iter().map(|item| item.printer).collect(),
                });
            } else if let Some(association) = associated.into_iter().next() {
                let installed = state
                    .printers
                    .iter()
                    .any(|p| normalize(&p.name) == normalize(&association.printer));
                let already_default = state
                    .printers
                    .iter()
                    .any(|p| p.is_default && normalize(&p.name) == normalize(&association.printer));
                if already_default {
                    state.last_prompted_network_key = Some(key.clone());
                    state.last_prompted_association_count = Some(1);
                } else if installed {
                    if matches!(state.config.prompt_mode, PromptMode::OncePerConnection) {
                        state.last_prompted_network_key = Some(key.clone());
                        state.last_prompted_association_count = Some(1);
                    }
                    prompt = Some(CheckPrompt::Switch {
                        network,
                        printer: association.printer,
                    });
                }
            }
        }
    }
    match prompt {
        Some(CheckPrompt::Duplicate { network, printers }) => {
            let printers = printers.join("\n");
            balloon(
                hwnd,
                "Duplicate printer associations found",
                &format!("Multiple printers are saved for {}.", network.name),
            );
            message(
                hwnd,
                &format!(
                    "Network: {}\n\nMultiple printer associations were found:\n{}\n\nOpen NetPrintSwitch settings and delete the extra association before switching.",
                    network.name, printers
                ),
                "NetPrintSwitch warning",
                MB_OK | MB_ICONWARNING | MB_TOPMOST,
            );
        }
        Some(CheckPrompt::Switch { network, printer }) => {
            balloon(
                hwnd,
                "Associated printer network found",
                &format!("{} is associated with {}.", network.name, printer),
            );
            let answer = message(
                hwnd,
                &format!(
                    "Network: {}\nAssociated printer: {}\n\nSwitch Windows default printer?",
                    network.name, printer
                ),
                "NetPrintSwitch",
                MB_YESNO | MB_ICONQUESTION | MB_TOPMOST,
            );
            if answer == windows::Win32::UI::WindowsAndMessaging::IDYES {
                match set_default_printer(&printer) {
                    Ok(()) => {
                        if refresh_printers_for_event().is_err() {
                            schedule_network_retry(hwnd);
                        }
                        balloon(hwnd, "Default printer switched", &printer);
                    }
                    Err(error) => {
                        STATE
                            .get()
                            .unwrap()
                            .lock()
                            .unwrap()
                            .last_prompted_network_key = None;
                        STATE
                            .get()
                            .unwrap()
                            .lock()
                            .unwrap()
                            .last_prompted_association_count = None;
                        message(
                            hwnd,
                            &format!("Could not switch default printer: {error}"),
                            "NetPrintSwitch",
                            MB_OK | MB_ICONERROR,
                        );
                    }
                }
            }
        }
        None => {}
    }
}

unsafe fn refresh_printers_for_event() -> Result<(), String> {
    let printers = match enumerate_printers_result() {
        Ok(printers) => printers,
        Err(error) => {
            STATE.get().unwrap().lock().unwrap().printers_loaded = false;
            return Err(error);
        }
    };
    let mut state = STATE.get().unwrap().lock().unwrap();
    state.printers = printers;
    state.printers_loaded = true;
    Ok(())
}

fn start_network_check(hwnd: HWND, refresh_printers: bool, connection_event: bool) {
    if refresh_printers {
        invalidate_printer_cache();
    }
    if CHECK_IN_PROGRESS.swap(true, std::sync::atomic::Ordering::AcqRel) {
        CHECK_PENDING.store(true, std::sync::atomic::Ordering::Release);
        return;
    }
    let hwnd_raw = hwnd.0 as usize;
    let spawn_result = thread::Builder::new()
        .name("network-check".to_string())
        .spawn(move || {
            let raw = Box::into_raw(Box::new(NetworkCheckResult {
                connection_event,
                network: detect_network_result(),
                printers: refresh_printers.then(enumerate_printers_result),
            }));
            unsafe {
                let hwnd = HWND(hwnd_raw as *mut std::ffi::c_void);
                if PostMessageW(
                    Some(hwnd),
                    WM_NETWORK_RESULT,
                    WPARAM(0),
                    LPARAM(raw as isize),
                )
                .is_err()
                {
                    drop(Box::from_raw(raw));
                    CHECK_IN_PROGRESS.store(false, std::sync::atomic::Ordering::Release);
                }
            }
        });
    if let Err(error) = spawn_result {
        CHECK_IN_PROGRESS.store(false, std::sync::atomic::Ordering::Release);
        report_check_error(
            HWND(hwnd_raw as *mut std::ffi::c_void),
            &format!("could not start network check: {error}"),
        );
        schedule_network_retry(HWND(hwnd_raw as *mut std::ffi::c_void));
    }
}

unsafe fn handle_network_result(hwnd: HWND, result: NetworkCheckResult) {
    CHECK_IN_PROGRESS.store(false, std::sync::atomic::Ordering::Release);
    let NetworkCheckResult {
        connection_event,
        network,
        printers,
    } = result;
    if let Some(printers) = printers {
        let printers = match printers {
            Ok(printers) => printers,
            Err(error) => {
                report_check_error(hwnd, &error);
                schedule_network_retry(hwnd);
                CHECK_PENDING.store(false, std::sync::atomic::Ordering::Release);
                return;
            }
        };
        let mut state = STATE.get().unwrap().lock().unwrap();
        state.printers = printers;
        state.printers_loaded = true;
        drop(state);
        refresh_ui(hwnd);
    }
    let network = match network {
        Ok(network) => network,
        Err(error) => {
            report_check_error(hwnd, &error);
            schedule_network_retry(hwnd);
            CHECK_PENDING.store(false, std::sync::atomic::Ordering::Release);
            return;
        }
    };
    RETRY_ATTEMPT.store(0, std::sync::atomic::Ordering::Release);
    RETRY_ERROR_NOTIFIED.store(false, std::sync::atomic::Ordering::Release);
    if PROMPT_IN_PROGRESS.swap(true, std::sync::atomic::Ordering::AcqRel) {
        CHECK_PENDING.store(true, std::sync::atomic::Ordering::Release);
        return;
    }
    match network {
        Some(network) => evaluate_network(hwnd, connection_event, network),
        None => clear_network_state(hwnd),
    }
    PROMPT_IN_PROGRESS.store(false, std::sync::atomic::Ordering::Release);
    if CHECK_PENDING.swap(false, std::sync::atomic::Ordering::AcqRel) {
        start_network_check(hwnd, true, true);
    }
}

fn schedule_network_retry(hwnd: HWND) {
    let retry_state = RETRY_SCHEDULED.get_or_init(|| Mutex::new(false));
    let mut scheduled = retry_state.lock().unwrap();
    if *scheduled {
        return;
    }
    *scheduled = true;
    drop(scheduled);
    let attempt = RETRY_ATTEMPT
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
        .saturating_add(1);
    let delay = retry_delay(attempt);
    let hwnd = hwnd.0 as usize;
    let spawn_result = thread::Builder::new()
        .name("network-check-retry".to_string())
        .spawn(move || {
            thread::sleep(delay);
            if let Some(state) = RETRY_SCHEDULED.get() {
                *state.lock().unwrap() = false;
            }
            unsafe {
                let hwnd = HWND(hwnd as *mut std::ffi::c_void);
                let _ = PostMessageW(Some(hwnd), WM_RETRY_NETWORK, WPARAM(0), LPARAM(0));
            }
        });
    if spawn_result.is_err() {
        if let Some(state) = RETRY_SCHEDULED.get() {
            *state.lock().unwrap() = false;
        }
        report_check_error(hwnd_from_usize(hwnd), "could not schedule a retry thread");
    }
}

fn hwnd_from_usize(value: usize) -> HWND {
    HWND(value as *mut std::ffi::c_void)
}

fn retry_delay(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(7);
    Duration::from_secs(2u64.pow(exponent + 1))
}

fn report_check_error(hwnd: HWND, error: &str) {
    if !RETRY_ERROR_NOTIFIED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        unsafe {
            balloon(
                hwnd,
                "Automatic check delayed",
                &format!("NetPrintSwitch will retry: {error}"),
            );
        }
    }
}

unsafe fn find_main_window() -> windows::core::Result<HWND> {
    let class_name = wide("NetPrintSwitchWindow");
    FindWindowW(PCWSTR(class_name.as_ptr()), PCWSTR::null())
}

fn find_main_window_with_retry() -> Option<HWND> {
    for _ in 0..100 {
        let Ok(window) = (unsafe { find_main_window() }) else {
            thread::sleep(Duration::from_millis(25));
            continue;
        };
        if !window.0.is_null() {
            return Some(window);
        }
        thread::sleep(Duration::from_millis(25));
    }
    None
}

fn remove_scheduled_task() -> Result<(), String> {
    let mut command = Command::new("schtasks.exe");
    command
        .creation_flags(CREATE_NO_WINDOW.0)
        .args(["/Delete", "/TN", TASK_NAME, "/F"]);
    let output = run_command_with_timeout(command, "scheduled task removal", COMMAND_TIMEOUT)?;
    if output.status.success() {
        Ok(())
    } else {
        let details = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let details = if details.is_empty() {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            details
        };
        if is_missing_task_error(&details) {
            return Ok(());
        }
        if details.is_empty() {
            Err(format!("Task Scheduler returned status {}", output.status))
        } else {
            Err(format!(
                "Task Scheduler returned status {}: {details}",
                output.status
            ))
        }
    }
}

fn is_missing_task_error(details: &str) -> bool {
    let details = details.to_ascii_lowercase();
    details.contains("cannot find")
        || details.contains("does not exist")
        || details.contains("not found")
}

fn install_scheduled_task() -> Result<(), String> {
    let executable = env::current_exe().map_err(|error| error.to_string())?;
    if scheduled_task_matches(&executable) {
        return Ok(());
    }
    let action = format!("\"{}\" --check-network", executable.display());
    let mut command = Command::new("schtasks.exe");
    command.creation_flags(CREATE_NO_WINDOW.0).args([
        "/Create",
        "/TN",
        TASK_NAME,
        "/SC",
        "ONEVENT",
        "/EC",
        NETWORK_LOG,
        "/MO",
        "*[System/EventID=10000]",
        "/TR",
        &action,
        "/IT",
        "/F",
    ]);
    let output = run_command_with_timeout(command, "scheduled task installation", COMMAND_TIMEOUT)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(command_output_error("scheduled task installation", &output))
    }
}

fn scheduled_task_matches(executable: &std::path::Path) -> bool {
    let mut command = Command::new("schtasks.exe");
    command
        .creation_flags(CREATE_NO_WINDOW.0)
        .args(["/Query", "/TN", TASK_NAME, "/XML"]);
    let Ok(output) = run_command_with_timeout(command, "scheduled task query", COMMAND_TIMEOUT)
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let xml = command_output_text(&output.stdout).to_ascii_lowercase();
    let expected = executable
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    xml.contains(&expected) && xml.contains("<arguments>--check-network</arguments>")
}

fn command_output_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

unsafe fn show_tray_menu(hwnd: HWND) {
    let menu = CreatePopupMenu().unwrap_or_default();
    let show = wide("Open NetPrintSwitch");
    let refresh = wide("Refresh status");
    let exit = wide("Exit");
    let _ = AppendMenuW(menu, MF_STRING, ID_SHOW, PCWSTR(show.as_ptr()));
    let _ = AppendMenuW(menu, MF_STRING, ID_REFRESH, PCWSTR(refresh.as_ptr()));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, PCWSTR(exit.as_ptr()));
    let mut point = POINT::default();
    let _ = GetCursorPos(&mut point);
    let _ = SetForegroundWindow(hwnd);
    let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, point.x, point.y, Some(0), hwnd, None);
    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);
}

unsafe fn add_tray_icon(hwnd: HWND) -> Result<(), String> {
    let (icon, icon_owned) = load_tray_icon();
    let mut data = tray_data(hwnd, true);
    data.hIcon = icon;
    if Shell_NotifyIconW(NIM_ADD, &data).as_bool() {
        let mut state = STATE.get().unwrap().lock().unwrap();
        state.tray_added = true;
        state.tray_icon = data.hIcon;
        state.tray_icon_owned = icon_owned;
        Ok(())
    } else if icon_owned && !data.hIcon.is_invalid() {
        let _ = DestroyIcon(data.hIcon);
        Err("Shell_NotifyIconW rejected tray icon registration".to_string())
    } else {
        Err("Shell_NotifyIconW rejected tray icon registration".to_string())
    }
}

unsafe fn load_tray_icon() -> (HICON, bool) {
    if let Some(icon_path) = ensure_icon_file() {
        let icon_path = wide(icon_path.to_string_lossy().as_ref());
        if let Ok(handle) = LoadImageW(
            None,
            PCWSTR(icon_path.as_ptr()),
            IMAGE_ICON,
            32,
            32,
            LR_LOADFROMFILE | LR_DEFAULTSIZE,
        ) {
            return (HICON(handle.0), true);
        }
    }
    (LoadIconW(None, IDI_APPLICATION).unwrap_or_default(), false)
}

unsafe fn tray_data(hwnd: HWND, with_icon: bool) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        ..Default::default()
    };
    if with_icon {
        data.uFlags |= NIF_ICON;
    }
    let tip = wide("NetPrintSwitch");
    copy_wide(&mut data.szTip, &tip);
    data
}

unsafe fn balloon(hwnd: HWND, title: &str, body: &str) {
    let mut data = tray_data(hwnd, false);
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = NIIF_INFO;
    let title = wide(title);
    let body = wide(body);
    copy_wide(&mut data.szInfoTitle, &title);
    copy_wide(&mut data.szInfo, &body);
    let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
}

const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);

fn run_command_with_timeout(
    mut command: Command,
    operation: &str,
    timeout: Duration,
) -> Result<Output, String> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child: Child = command
        .spawn()
        .map_err(|error| format!("could not start {operation}: {error}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|error| format!("could not collect {operation} output: {error}"));
            }
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{operation} timed out after {} seconds",
                    timeout.as_secs()
                ));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("could not monitor {operation}: {error}"));
            }
        }
    }
}

fn detect_network_result() -> Result<Option<Network>, String> {
    let script = r#"
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$profiles = @(Get-NetConnectionProfile -ErrorAction Stop | Where-Object {
    $_.Name -and (($_.IPv4Connectivity -ne 'Disconnected') -or ($_.IPv6Connectivity -ne 'Disconnected'))
})
$profiles | ForEach-Object {
    $adapter = Get-NetAdapter -InterfaceIndex $_.InterfaceIndex -ErrorAction Stop
    if ($null -ne $adapter) {
        $medium = [int]$adapter.NdisPhysicalMedium
        $route = @(Get-NetRoute -InterfaceIndex $_.InterfaceIndex -ErrorAction SilentlyContinue | Where-Object {
            $_.DestinationPrefix -in @('0.0.0.0/0', '::/0')
        } | Sort-Object RouteMetric | Select-Object -First 1)
        $networkType = switch ($medium) {
            { $_ -in @(1, 9) } { 'Wi-Fi'; break }
            { $_ -eq 10 } { 'Bluetooth'; break }
            { $_ -in @(8, 12) } { 'Cellular'; break }
            default { 'Ethernet' }
        }
        [pscustomobject]@{
            Name = $_.Name
            NetworkType = $networkType
            HasDefaultRoute = $route.Count -gt 0
            RouteMetric = if ($route.Count -gt 0) { [int]$route[0].RouteMetric } else { [int]::MaxValue }
            InterfaceIndex = [int]$_.InterfaceIndex
        }
    }
} | Sort-Object @{Expression={ if ($_.HasDefaultRoute) { 0 } else { 1 } }}, RouteMetric, InterfaceIndex |
    Select-Object -First 1 | ConvertTo-Json -Compress
"#;
    let mut command = Command::new("powershell.exe");
    command.creation_flags(CREATE_NO_WINDOW.0).args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        script,
    ]);
    let output = run_command_with_timeout(command, "network detection", COMMAND_TIMEOUT)?;
    if !output.status.success() {
        return Err(command_output_error("network detection", &output));
    }
    if output.stdout.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    let value = serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .map_err(|error| format!("network detection returned invalid JSON: {error}"))?;
    let name = value
        .get("Name")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "network detection returned no network name".to_string())?;
    let network_type = value
        .get("NetworkType")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|network_type| !network_type.is_empty())
        .ok_or_else(|| "network detection returned no network type".to_string())?;
    Ok(Some(Network {
        name: name.to_string(),
        network_type: network_type.to_string(),
    }))
}

fn enumerate_printers_result() -> Result<Vec<Printer>, String> {
    if let Some(cache) = PRINTER_CACHE.get() {
        if let Some((updated, printers)) = cache.lock().unwrap().as_ref() {
            if updated.elapsed() < Duration::from_secs(2) {
                return Ok(printers.clone());
            }
        }
    }
    let script = r#"
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
Get-CimInstance Win32_Printer -ErrorAction Stop |
    Select-Object Name,Default |
    ConvertTo-Json -Compress
"#;
    let mut command = Command::new("powershell.exe");
    command.creation_flags(CREATE_NO_WINDOW.0).args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        script,
    ]);
    let output = run_command_with_timeout(command, "printer enumeration", COMMAND_TIMEOUT)?;
    if !output.status.success() {
        return Err(command_output_error("printer enumeration", &output));
    }
    if output.stdout.iter().all(u8::is_ascii_whitespace) {
        return Ok(Vec::new());
    }
    let value = serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .map_err(|error| format!("printer enumeration returned invalid JSON: {error}"))?;
    let items = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(_) => vec![value],
        _ => return Err("printer enumeration returned unexpected JSON".to_string()),
    };
    let printers: Vec<Printer> = items
        .into_iter()
        .filter_map(|item| {
            let name = item.get("Name")?.as_str()?.to_string();
            Some(Printer {
                name,
                is_default: item
                    .get("Default")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            })
        })
        .collect();
    *PRINTER_CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some((Instant::now(), printers.clone()));
    Ok(printers)
}

fn invalidate_printer_cache() {
    if let Some(cache) = PRINTER_CACHE.get() {
        *cache.lock().unwrap() = None;
    }
}

fn command_output_error(operation: &str, output: &Output) -> String {
    let details = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let details = if details.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        details
    };
    if details.is_empty() {
        format!("{operation} failed with status {}", output.status)
    } else {
        format!(
            "{operation} failed with status {}: {details}",
            output.status
        )
    }
}

pub(crate) fn set_default_printer(printer: &str) -> Result<(), String> {
    let mut command = Command::new("rundll32.exe");
    command.creation_flags(CREATE_NO_WINDOW.0).args([
        "printui.dll,PrintUIEntry",
        "/y",
        "/n",
        printer,
    ]);
    let output = run_command_with_timeout(command, "default printer change", COMMAND_TIMEOUT)?;
    if output.status.success() {
        invalidate_printer_cache();
        let printers = enumerate_printers_result()
            .map_err(|error| format!("printer changed, but verification failed: {error}"))?;
        if printers
            .iter()
            .any(|item| item.is_default && normalize(&item.name) == normalize(printer))
        {
            Ok(())
        } else {
            Err(
                "Windows reported success, but the requested printer is not the default"
                    .to_string(),
            )
        }
    } else {
        Err(command_output_error("default printer change", &output))
    }
}

fn config_path() -> PathBuf {
    let mut path = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    path.push("NetPrintSwitch");
    path.push("config.json");
    path
}

pub(crate) fn load_config_result() -> Result<Config, String> {
    with_config_lock(load_config_unlocked)
}

fn load_config_unlocked() -> Result<Config, String> {
    let path = config_path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Config::default());
        }
        Err(error) => return Err(format!("could not read configuration: {error}")),
    };
    match serde_json::from_str(&text) {
        Ok(config) => Ok(config),
        Err(error) => {
            let stamp = fs::metadata(&path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_nanos())
                .unwrap_or_default();
            let backup = path.with_file_name(format!("config.json.corrupt-{stamp}"));
            let backup_error = if backup.exists() {
                None
            } else {
                fs::copy(&path, &backup).err()
            };
            let detail = backup_error
                .map(|error| format!(" Could not preserve corrupt file: {error}."))
                .unwrap_or_default();
            Err(format!(
                "configuration is invalid: {error}. Backup: {}.{detail}",
                backup.display()
            ))
        }
    }
}

pub(crate) fn save_config(config: &Config) -> Result<(), String> {
    with_config_lock(|| save_config_unlocked(config))
}

fn save_config_unlocked(config: &Config) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    let temp_path = path.with_file_name(format!("config.json.tmp-{}", process::id()));
    fs::write(&temp_path, text).map_err(|error| error.to_string())?;
    let source = wide(temp_path.to_string_lossy().as_ref());
    let destination = wide(path.to_string_lossy().as_ref());
    let result = unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if let Err(error) = result {
        let _ = fs::remove_file(&temp_path);
        return Err(error.to_string());
    }
    Ok(())
}

fn with_config_lock<T>(operation: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let name = wide("Local\\NetPrintSwitch.Config");
    let mutex = unsafe {
        CreateMutexW(None, false, PCWSTR(name.as_ptr()))
            .map_err(|error| format!("could not create configuration lock: {error}"))?
    };
    let wait = unsafe { WaitForSingleObject(mutex, INFINITE) };
    if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
        let _ = unsafe { CloseHandle(mutex) };
        return Err("could not acquire configuration lock".to_string());
    }
    let result = operation();
    let _ = unsafe { ReleaseMutex(mutex) };
    let _ = unsafe { CloseHandle(mutex) };
    result
}

const ICON_BYTES: &[u8] = include_bytes!("../PrintSwitch.ico");

pub(crate) fn ensure_icon_file() -> Option<PathBuf> {
    let mut icon_path = config_path();
    icon_path.set_file_name("PrintSwitch.ico");
    if let Some(parent) = icon_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let needs_update = fs::read(&icon_path)
        .map(|bytes| bytes.as_slice() != ICON_BYTES)
        .unwrap_or(true);
    if needs_update && fs::write(&icon_path, ICON_BYTES).is_err() {
        if let Ok(executable) = env::current_exe() {
            let installed_path = executable.with_file_name("PrintSwitch.ico");
            if installed_path.is_file() {
                return Some(installed_path);
            }
        }
        let source_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("PrintSwitch.ico");
        return source_path.is_file().then_some(source_path);
    }
    Some(icon_path)
}

unsafe fn get_text(hwnd: HWND) -> String {
    let length = GetWindowTextLengthW(hwnd);
    if length <= 0 {
        return String::new();
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let read = GetWindowTextW(hwnd, &mut buffer);
    String::from_utf16_lossy(&buffer[..read as usize])
}

unsafe fn message(
    hwnd: HWND,
    text: &str,
    title: &str,
    flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE,
) -> windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_RESULT {
    let text = wide(text);
    let title = wide(title);
    windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
        Some(hwnd),
        PCWSTR(text.as_ptr()),
        PCWSTR(title.as_ptr()),
        flags,
    )
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}

fn association_matches(association: &Association, network: &Network) -> bool {
    normalize(&association.network) == normalize(&network.name)
        && (association.network_type.eq_ignore_ascii_case("Network")
            || association
                .network_type
                .eq_ignore_ascii_case(&network.network_type))
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn copy_wide<const N: usize>(target: &mut [u16; N], source: &[u16]) {
    let count = source.len().saturating_sub(1).min(N.saturating_sub(1));
    target[..count].copy_from_slice(&source[..count]);
    target[count] = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_uses_safe_close_default() {
        let config: Config = serde_json::from_str(r#"{"associations":[]}"#).unwrap();
        assert_eq!(config.close_behavior, CloseBehavior::AskOncePerStartup);
    }

    #[test]
    fn retry_delay_backs_off_and_is_bounded() {
        assert_eq!(retry_delay(1), Duration::from_secs(2));
        assert_eq!(retry_delay(2), Duration::from_secs(4));
        assert_eq!(retry_delay(9), Duration::from_secs(256));
        assert_eq!(retry_delay(u32::MAX), Duration::from_secs(256));
    }

    #[test]
    fn associations_match_network_case_insensitively() {
        let network = Network {
            name: "Office Wi-Fi".to_string(),
            network_type: "Wi-Fi".to_string(),
        };
        let association = Association {
            network: " office wi-fi ".to_string(),
            network_type: "wI-Fi".to_string(),
            printer: "Office Printer".to_string(),
        };
        assert!(association_matches(&association, &network));
    }

    #[test]
    fn task_output_supports_utf16_and_utf8() {
        assert_eq!(
            command_output_text(b"<Arguments>--check-network</Arguments>"),
            "<Arguments>--check-network</Arguments>"
        );
        assert_eq!(command_output_text(&[0xff, 0xfe, b'A', 0, b'B', 0]), "AB");
    }

    #[test]
    fn timed_command_collects_child_output() {
        let mut command = Command::new("cmd.exe");
        command.args(["/C", "echo printer"]);
        let output = run_command_with_timeout(command, "test command", Duration::from_secs(2))
            .expect("timed command should succeed");
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "printer");
    }
}
