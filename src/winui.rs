// NetPrintSwitch
// Copyright (C) 2026 fattminer
// SPDX-License-Identifier: LicenseRef-NetPrintSwitch-AGPL-3.0-only-PLUS-Commons-Clause-1.0

use super::{
    detect_network_result, ensure_icon_file, enumerate_printers_result, load_config_result,
    normalize, save_config, Association, CloseBehavior, Config, Network, Printer, PromptMode,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    mem, process,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
    thread,
    time::Duration,
};
use windows::{
    core::{BOOL, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::WindowsAndMessaging::{
            CallWindowProcW, DefWindowProcW, EnumWindows, FindWindowW, GetWindowTextLengthW,
            GetWindowTextW, GetWindowThreadProcessId, MessageBoxW, PostMessageW,
            SetForegroundWindow, SetWindowLongPtrW, ShowWindow, GWLP_WNDPROC, IDNO, IDYES,
            MB_ICONQUESTION, MB_TOPMOST, MB_YESNO, SW_RESTORE, WM_CLOSE, WNDPROC,
        },
    },
};
use windows_reactor::*;

const SURFACE: Color = Color::rgb(243, 243, 243);
const CARD: Color = Color::rgb(255, 255, 255);
const STROKE: Color = Color::rgb(225, 225, 225);
const INPUT_STROKE: Color = Color::rgb(117, 117, 117);
const TEXT: Color = Color::rgb(32, 32, 32);
const MUTED: Color = Color::rgb(96, 96, 96);
const ACCENT: Color = Color::rgb(0, 95, 184);

static ICON_PATH: OnceLock<Option<&'static str>> = OnceLock::new();
static ORIGINAL_WND_PROC: OnceLock<(usize, WNDPROC)> = OnceLock::new();
static CLOSE_INTERCEPTOR_STARTED: AtomicBool = AtomicBool::new(false);
static CLOSE_BEHAVIOR: OnceLock<Mutex<CloseBehavior>> = OnceLock::new();
static CLOSE_DECISION: OnceLock<Mutex<Option<CloseAction>>> = OnceLock::new();

thread_local! {
    static UI_REFRESH_CALLBACK: RefCell<Option<Callback<()>>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy)]
enum CloseAction {
    Minimize,
    Quit,
}

pub(crate) fn run() -> Result<(), String> {
    install_close_interceptor();
    App::run_component::<NetPrintSwitch>(()).map_err(|error| error.to_string())
}

fn install_close_interceptor() {
    if CLOSE_INTERCEPTOR_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    if thread::Builder::new()
        .name("window-close-interceptor".to_string())
        .spawn(|| {
            // Component creation can take several seconds while printers and
            // network state are queried. Keep looking until the native window
            // is available instead of silently losing the close confirmation.
            for _ in 0..600 {
                if let Some(hwnd) = find_ui_window() {
                    let previous = unsafe {
                        SetWindowLongPtrW(
                            hwnd,
                            GWLP_WNDPROC,
                            close_interceptor as *const () as usize as isize,
                        )
                    };
                    if previous != 0 {
                        let previous = unsafe { mem::transmute::<isize, WNDPROC>(previous) };
                        let _ = ORIGINAL_WND_PROC.set((hwnd.0 as usize, previous));
                        return;
                    }
                }
                thread::sleep(Duration::from_millis(50));
            }
            unsafe { notify_close_interceptor_failure() };
        })
        .is_err()
    {
        CLOSE_INTERCEPTOR_STARTED.store(false, Ordering::Release);
    }
}

unsafe fn notify_close_interceptor_failure() {
    let class_name = wide("NetPrintSwitchWindow");
    if let Ok(hwnd) = FindWindowW(PCWSTR(class_name.as_ptr()), PCWSTR::null()) {
        if !hwnd.0.is_null() {
            let _ = PostMessageW(Some(hwnd), super::WM_UI_HOOK_FAILED, WPARAM(0), LPARAM(0));
        }
    }
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
    if process_id != process::id() {
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

unsafe extern "system" fn close_interceptor(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == super::WM_UI_SHOW {
        refresh_ui();
        let _ = ShowWindow(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
        return LRESULT(0);
    }
    if message == super::WM_UI_REFRESH {
        refresh_ui();
        return LRESULT(0);
    }
    if message == super::WM_UI_EXIT {
        return call_original(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
    }
    if message == WM_CLOSE {
        let action = match configured_close_behavior() {
            CloseBehavior::AlwaysMinimize => CloseAction::Minimize,
            CloseBehavior::AlwaysQuit => CloseAction::Quit,
            CloseBehavior::AskOncePerStartup => {
                let decision = CLOSE_DECISION
                    .get_or_init(|| Mutex::new(None))
                    .lock()
                    .expect("close decision lock poisoned");
                if let Some(action) = *decision {
                    action
                } else {
                    drop(decision);
                    let prompt = "NetPrintSwitch must be running to switch your printer when you change networks.\n\nClosing NetPrintSwitch will stop automatic network monitoring. Are you sure you want to close it?\n\nYes: Quit NetPrintSwitch completely\nNo: Minimize to the system tray\n\nYour choice will be remembered until NetPrintSwitch restarts.";
                    let title = "Close NetPrintSwitch?";
                    let result = MessageBoxW(
                        Some(hwnd),
                        PCWSTR(wide(prompt).as_ptr()),
                        PCWSTR(wide(title).as_ptr()),
                        MB_YESNO | MB_ICONQUESTION | MB_TOPMOST,
                    );
                    let Some(action) = (match result {
                        IDYES => Some(CloseAction::Quit),
                        IDNO => Some(CloseAction::Minimize),
                        _ => None,
                    }) else {
                        return LRESULT(0);
                    };
                    *CLOSE_DECISION
                        .get_or_init(|| Mutex::new(None))
                        .lock()
                        .expect("close decision lock poisoned") = Some(action);
                    action
                }
            }
        };
        match action {
            CloseAction::Quit => {
                request_application_exit();
                return call_original(hwnd, message, wparam, lparam);
            }
            CloseAction::Minimize => {
                let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_HIDE);
                return LRESULT(0);
            }
        }
    }
    call_original(hwnd, message, wparam, lparam)
}

unsafe fn call_original(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some((original_hwnd, original)) = ORIGINAL_WND_PROC.get() {
        if *original_hwnd == hwnd.0 as usize {
            return CallWindowProcW(*original, hwnd, message, wparam, lparam);
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    } else {
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

unsafe fn request_application_exit() {
    let class_name = wide("NetPrintSwitchWindow");
    if let Ok(hwnd) = FindWindowW(PCWSTR(class_name.as_ptr()), PCWSTR::null()) {
        if !hwnd.0.is_null() {
            let _ = PostMessageW(Some(hwnd), super::WM_EXIT_APP, WPARAM(0), LPARAM(0));
        }
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn configured_close_behavior() -> CloseBehavior {
    CLOSE_BEHAVIOR
        .get_or_init(|| Mutex::new(CloseBehavior::default()))
        .lock()
        .expect("close behavior lock poisoned")
        .clone()
}

fn set_close_behavior(value: CloseBehavior) {
    *CLOSE_BEHAVIOR
        .get_or_init(|| Mutex::new(CloseBehavior::default()))
        .lock()
        .expect("close behavior lock poisoned") = value;
    *CLOSE_DECISION
        .get_or_init(|| Mutex::new(None))
        .lock()
        .expect("close decision lock poisoned") = None;
}

fn refresh_ui() {
    UI_REFRESH_CALLBACK.with(|slot| {
        if let Some(callback) = slot.borrow().as_ref() {
            let _ = callback.call(());
        }
    });
}

struct NetPrintSwitch {
    config: Config,
    printers: Vec<Printer>,
    current_network: Option<Network>,
    network_name: String,
    selected_printer: Option<usize>,
    prompt_mode: PromptMode,
    status: String,
    data_loading: bool,
}

#[derive(Clone)]
enum Message {
    NetworkChanged(String),
    PrinterChanged(Option<usize>),
    PromptModeChanged(Option<usize>),
    CloseBehaviorChanged(Option<usize>),
    SaveAssociation,
    DeleteAssociation(usize),
    Refresh,
    InitialDataLoaded {
        config: Result<Config, String>,
        printers: Result<Vec<Printer>, String>,
        network: Result<Option<Network>, String>,
    },
    DataRefreshed {
        config: Result<Config, String>,
        printers: Result<Vec<Printer>, String>,
        network: Result<Option<Network>, String>,
    },
}

impl Component for NetPrintSwitch {
    type Input = ();
    type Message = Message;

    fn create(_input: &(), context: &ComponentContext<Self>) -> Self {
        let (config, config_error) = match load_config_result() {
            Ok(config) => (config, None),
            Err(error) => (Config::default(), Some(error)),
        };
        let _ = CLOSE_BEHAVIOR.set(Mutex::new(config.close_behavior.clone()));
        let prompt_mode = config.prompt_mode.clone();
        let component = Self {
            config,
            printers: Vec::new(),
            current_network: None,
            network_name: String::new(),
            selected_printer: None,
            prompt_mode,
            status: config_error
                .unwrap_or_else(|| "Loading printers and network status…".to_string()),
            data_loading: true,
        };
        context.spawn_background(|_| Message::InitialDataLoaded {
            config: load_config_result(),
            printers: enumerate_printers_result(),
            network: detect_network_result(),
        });
        component
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::NetworkChanged(value) => self.network_name = value,
            Message::PrinterChanged(index) => self.selected_printer = index,
            Message::PromptModeChanged(index) => {
                let prompt_mode = if index == Some(1) {
                    PromptMode::EveryConnection
                } else {
                    PromptMode::OncePerConnection
                };
                let Ok(mut config) = load_config_result() else {
                    self.status = "Could not read configuration before saving.".to_string();
                    return;
                };
                config.prompt_mode = prompt_mode.clone();
                match save_config(&config) {
                    Ok(()) => {
                        self.prompt_mode = prompt_mode;
                        self.config = config;
                        self.status = "Prompt preference saved.".to_string();
                    }
                    Err(error) => {
                        self.status = format!("Could not save prompt preference: {error}");
                    }
                }
            }
            Message::CloseBehaviorChanged(index) => {
                let close_behavior = match index {
                    Some(1) => CloseBehavior::AlwaysMinimize,
                    Some(2) => CloseBehavior::AlwaysQuit,
                    _ => CloseBehavior::AskOncePerStartup,
                };
                let Ok(mut config) = load_config_result() else {
                    self.status = "Could not read configuration before saving.".to_string();
                    return;
                };
                config.close_behavior = close_behavior.clone();
                match save_config(&config) {
                    Ok(()) => {
                        set_close_behavior(close_behavior);
                        self.config = config;
                        self.status = "Close behavior saved.".to_string();
                    }
                    Err(error) => {
                        self.status = format!("Could not save close behavior: {error}");
                    }
                }
            }
            Message::SaveAssociation => self.save_association(),
            Message::DeleteAssociation(index) => {
                let Ok(mut config) = load_config_result() else {
                    self.status = "Could not read configuration before saving.".to_string();
                    return;
                };
                if index < config.associations.len() {
                    config.associations.remove(index);
                    match save_config(&config) {
                        Ok(()) => {
                            self.config = config;
                            self.status = "Association removed.".to_string();
                        }
                        Err(error) => {
                            self.status = format!("Could not save changes: {error}");
                        }
                    }
                } else {
                    self.status = "Association list changed; refresh and try again.".to_string();
                }
            }
            Message::Refresh => {
                if self.data_loading {
                    return;
                }
                self.data_loading = true;
                self.status = "Refreshing printers and network status…".to_string();
                context.spawn_background(|_| Message::DataRefreshed {
                    config: load_config_result(),
                    printers: enumerate_printers_result(),
                    network: detect_network_result(),
                });
            }
            Message::InitialDataLoaded {
                config,
                printers,
                network,
            } => {
                self.data_loading = false;
                self.apply_data(config, printers, network, "Ready.");
            }
            Message::DataRefreshed {
                config,
                printers,
                network,
            } => {
                self.data_loading = false;
                self.apply_data(
                    config,
                    printers,
                    network,
                    "Printer and network list refreshed.",
                );
            }
        }
    }

    fn view(&self, _input: &(), context: &mut ViewContext<Self>) -> View {
        context.window_title("NetPrintSwitch");
        UI_REFRESH_CALLBACK.with(|slot| {
            *slot.borrow_mut() = Some(context.message(Message::Refresh));
        });
        let icon_path = ICON_PATH.get_or_init(|| {
            ensure_icon_file().map(|path| {
                Box::leak(path.to_string_lossy().into_owned().into_boxed_str()) as &'static str
            })
        });
        let mut visuals = WindowVisuals::new()
            .theme(WindowTheme::Light)
            .client_size(1000.0, 680.0)
            .constraints(WindowConstraints {
                min_width: Some(820.0),
                min_height: Some(560.0),
                ..Default::default()
            });
        if let Some(icon_path) = icon_path {
            visuals = visuals.icon(icon_path);
        }
        context.window_visuals(visuals);

        let network_type = self
            .current_network
            .as_ref()
            .map(|network| network.network_type.as_str())
            .unwrap_or("Not detected");
        let network_summary = if self.network_name.is_empty() {
            "No active network detected".to_string()
        } else {
            format!("{} · {}", self.network_name, network_type)
        };

        let printer_items = self.printers.iter().map(|printer| {
            if printer.is_default {
                format!("{} (current default)", printer.name)
            } else {
                printer.name.clone()
            }
        });
        let prompt_items = [
            "Once per connection".to_string(),
            "Every connection".to_string(),
        ];
        let prompt_index = match self.prompt_mode {
            PromptMode::OncePerConnection => 0,
            PromptMode::EveryConnection => 1,
        };
        let close_behavior_items = [
            "Ask once per startup".to_string(),
            "Always minimize to tray".to_string(),
            "Always quit NetPrintSwitch".to_string(),
        ];
        let close_behavior_index = match self.config.close_behavior {
            CloseBehavior::AskOncePerStartup => 0,
            CloseBehavior::AlwaysMinimize => 1,
            CloseBehavior::AlwaysQuit => 2,
        };

        let mut row_occurrences = HashMap::new();
        let saved_rows = self
            .config
            .associations
            .iter()
            .enumerate()
            .map(|(index, association)| {
                let base_key = format!(
                    "{}::{}::{}",
                    normalize(&association.network),
                    normalize(&association.network_type),
                    normalize(&association.printer)
                );
                let occurrence = row_occurrences.entry(base_key.clone()).or_insert(0);
                *occurrence += 1;
                let row_key = format!("{base_key}::{occurrence}");
                (
                    row_key,
                    Border::new()
                        .padding(Thickness::xy(12.0, 10.0))
                        .border_thickness(Thickness::uniform(1.0))
                        .border_brush(STROKE)
                        .corner_radius(6.0)
                        .background(CARD)
                        .content(
                            Grid::new()
                                .columns([GridLength::STAR, GridLength::Auto])
                                .column_spacing(12.0)
                                .children((
                                    StackPanel::new().spacing(3.0).children((
                                        TextBlock::new()
                                            .text(&association.network)
                                            .font_weight(FontWeight::SEMI_BOLD)
                                            .foreground(TEXT),
                                        TextBlock::new()
                                            .text(format!(
                                                "{} · {}",
                                                association.network_type, association.printer
                                            ))
                                            .font_size(13.0)
                                            .foreground(MUTED),
                                    )),
                                    Button::new()
                                        .grid_column(1)
                                        .on_click(
                                            context.message(Message::DeleteAssociation(index)),
                                        )
                                        .content("Delete"),
                                )),
                        ),
                )
            });

        let saved_content: View = if self.config.associations.is_empty() {
            TextBlock::new()
                .text("No associations saved yet.")
                .foreground(MUTED)
                .into()
        } else {
            StackPanel::new().spacing(8.0).keyed_children(saved_rows)
        };

        let content = ScrollViewer::new().content(
            Grid::new()
                .rows([GridLength::Auto, GridLength::STAR, GridLength::Auto])
                .row_spacing(18.0)
                .margin(Thickness::new(28.0, 24.0, 28.0, 28.0))
                .children((
                    StackPanel::new()
                        .grid_row(0)
                        .grid_column_span(2)
                        .spacing(5.0)
                        .children((
                            TextBlock::new()
                                .text("NetPrintSwitch")
                                .font_size(30.0)
                                .font_weight(FontWeight::BOLD)
                                .foreground(TEXT),
                            TextBlock::new()
                                .text("Switch your default printer when you change networks.")
                                .font_size(15.0)
                                .foreground(MUTED),
                        )),
                    Grid::new()
                        .grid_row(1)
                        .columns([GridLength::STAR, GridLength::STAR])
                        .column_spacing(18.0)
                        .children((
                            StackPanel::new()
                                .grid_column(0)
                                .spacing(18.0)
                                .children((
                                    card(
                                        "Current network",
                                        "The network detected by Windows right now.",
                                        StackPanel::new().spacing(4.0).children((
                                            TextBlock::new()
                                                .text(network_summary)
                                                .font_size(18.0)
                                                .font_weight(FontWeight::SEMI_BOLD)
                                                .foreground(TEXT),
                                            TextBlock::new()
                                                .text("Network changes are checked automatically in the background.")
                                                .font_size(13.0)
                                                .foreground(MUTED),
                                        )),
                                    ),
                                    card(
                                        "Create an association",
                                        "Choose the printer NetPrintSwitch should use on a network.",
                                        StackPanel::new().spacing(12.0).children((
                                            TextBlock::new()
                                                .text("Network name")
                                                .font_weight(FontWeight::SEMI_BOLD)
                                                .foreground(TEXT),
                                            TextBox::new()
                                                .text(&self.network_name)
                                                .placeholder_text("Example: Studio Wi-Fi")
                                                .background(CARD)
                                                .border_brush(INPUT_STROKE)
                                                .border_thickness(Thickness::uniform(1.0))
                                                .on_text_changed(context.callback(Message::NetworkChanged)),
                                            TextBlock::new()
                                                .text("Printer")
                                                .font_weight(FontWeight::SEMI_BOLD)
                                                .foreground(TEXT),
                                            input_frame(
                                                ComboBox::new()
                                                    .items_source(printer_items)
                                                    .selected_index(self.selected_printer)
                                                    .placeholder_text("Select a printer")
                                                    .on_selection_changed(context.callback(Message::PrinterChanged)),
                                            ),
                                            Button::new()
                                                .on_click(context.message(Message::SaveAssociation))
                                                .content("Save association"),
                                        )),
                                    ),
                                )),
                            StackPanel::new()
                                .grid_column(1)
                                .spacing(18.0)
                                .children((
                                    Border::new()
                                        .padding(Thickness::uniform(16.0))
                                        .border_thickness(Thickness::uniform(1.0))
                                        .border_brush(STROKE)
                                        .corner_radius(8.0)
                                        .background(CARD)
                                        .content(
                                            StackPanel::new().spacing(12.0).children((
                                                TextBlock::new()
                                                    .text("Saved associations")
                                                    .font_size(18.0)
                                                    .font_weight(FontWeight::SEMI_BOLD)
                                                    .foreground(TEXT),
                                                saved_content,
                                            )),
                                        ),
                                    card(
                                        "Behavior",
                                        "Control prompts and what happens when the window closes.",
                                        StackPanel::new().spacing(12.0).children((
                                            TextBlock::new()
                                                .text("Prompt frequency")
                                                .font_weight(FontWeight::SEMI_BOLD)
                                                .foreground(TEXT),
                                            input_frame(
                                                ComboBox::new()
                                                    .items_source(prompt_items)
                                                    .selected_index(prompt_index)
                                                    .on_selection_changed(context.callback(Message::PromptModeChanged)),
                                            ),
                                            TextBlock::new()
                                                .text("Close behavior")
                                                .font_weight(FontWeight::SEMI_BOLD)
                                                .foreground(TEXT),
                                            input_frame(
                                                ComboBox::new()
                                                    .items_source(close_behavior_items)
                                                    .selected_index(close_behavior_index)
                                                    .on_selection_changed(context.callback(Message::CloseBehaviorChanged)),
                                            ),
                                        )),
                                    ),
                                )),
                        )),
                    StackPanel::new()
                        .grid_row(2)
                        .grid_column_span(2)
                        .spacing(8.0)
                        .children((
                            Button::new()
                                .on_click(context.message(Message::Refresh))
                                .content("Refresh printers and network"),
                            TextBlock::new()
                                .text(&self.status)
                                .font_size(13.0)
                                .foreground(if self.status.starts_with("Could not")
                                    || self.status.contains("failed")
                                    || self.status.contains("invalid")
                                    || self.status.contains("unavailable")
                                {
                                    Color::rgb(196, 43, 28)
                                } else {
                                    ACCENT
                                }),
                        )),
                )),
        );
        Grid::new().background(SURFACE).children((content,))
    }
}

impl NetPrintSwitch {
    fn apply_data(
        &mut self,
        config: Result<Config, String>,
        printers: Result<Vec<Printer>, String>,
        network: Result<Option<Network>, String>,
        success_status: &str,
    ) {
        let mut errors = Vec::new();
        match config {
            Ok(config) => {
                self.prompt_mode = config.prompt_mode.clone();
                if configured_close_behavior() != config.close_behavior {
                    set_close_behavior(config.close_behavior.clone());
                }
                self.config = config;
            }
            Err(error) => errors.push(error),
        }
        match printers {
            Ok(printers) => {
                self.selected_printer = printers
                    .iter()
                    .position(|printer| printer.is_default)
                    .or_else(|| (!printers.is_empty()).then_some(0));
                self.printers = printers;
            }
            Err(error) => errors.push(error),
        }
        match network {
            Ok(network) => {
                self.network_name = network
                    .as_ref()
                    .map(|network| network.name.clone())
                    .unwrap_or_default();
                self.current_network = network;
            }
            Err(error) => errors.push(error),
        }
        self.status = if errors.is_empty() {
            success_status.to_string()
        } else {
            format!("Could not refresh status: {}", errors.join(" "))
        };
    }

    fn save_association(&mut self) {
        let network = self.network_name.trim();
        if network.is_empty() {
            self.status = "Enter a network name first.".to_string();
            return;
        }
        let Some(printer_index) = self.selected_printer else {
            self.status = "Select a printer first.".to_string();
            return;
        };
        let Some(printer) = self.printers.get(printer_index) else {
            self.status = "The selected printer is no longer available.".to_string();
            return;
        };
        let network_type = self
            .current_network
            .as_ref()
            .filter(|current| current.name.eq_ignore_ascii_case(network))
            .map(|current| current.network_type.clone())
            .unwrap_or_else(|| "Network".to_string());
        let association = Association {
            network: network.to_string(),
            network_type,
            printer: printer.name.clone(),
        };
        let association_type = association.network_type.clone();
        let Ok(mut config) = load_config_result() else {
            self.status = "Could not read configuration before saving.".to_string();
            return;
        };
        config.associations.retain(|existing| {
            !(normalize(&existing.network) == normalize(network)
                && (existing
                    .network_type
                    .eq_ignore_ascii_case(&association_type)
                    || existing.network_type.eq_ignore_ascii_case("Network")
                    || association_type.eq_ignore_ascii_case("Network")))
        });
        config.associations.push(association);
        match save_config(&config) {
            Ok(()) => {
                self.config = config;
                self.status = "Association saved.".to_string();
            }
            Err(error) => {
                self.status = format!("Could not save association: {error}");
            }
        }
    }
}

fn card(title: &str, description: &str, content: impl Into<View>) -> View {
    Border::new()
        .padding(Thickness::uniform(16.0))
        .border_thickness(Thickness::uniform(1.0))
        .border_brush(STROKE)
        .corner_radius(8.0)
        .background(CARD)
        .content(
            StackPanel::new().spacing(10.0).children((
                TextBlock::new()
                    .text(title)
                    .font_size(18.0)
                    .font_weight(FontWeight::SEMI_BOLD)
                    .foreground(TEXT),
                TextBlock::new()
                    .text(description)
                    .font_size(13.0)
                    .foreground(MUTED),
                content,
            )),
        )
}

fn input_frame(content: impl Into<View>) -> View {
    Border::new()
        .background(CARD)
        .corner_radius(4.0)
        .content(content)
}
