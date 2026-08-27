use std::{fs, path::Path, process::Command};

use eframe::egui;

pub(super) fn load_logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/sundial.png"))
        .expect("embedded Sundial logo must be a valid PNG");
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    ctx.load_texture("sundial-logo", image, egui::TextureOptions::LINEAR)
}

#[cfg(target_os = "linux")]
pub(super) fn load_linux_title_bar_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!(
        "../../assets/linux/io.github.kylethmpsn.Sundial-window.png"
    ))
    .expect("embedded Sundial title bar icon must be a valid PNG");
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [icon.width as usize, icon.height as usize],
        &icon.rgba,
    );
    ctx.load_texture(
        "sundial-title-bar-icon",
        image,
        egui::TextureOptions::LINEAR,
    )
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
enum LinuxTitleBarButton {
    Minimize,
    Maximize,
    Restore,
    Close,
}

#[cfg(target_os = "linux")]
fn linux_title_bar_button(ui: &mut egui::Ui, button: LinuxTitleBarButton) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(42.0, 36.0), egui::Sense::click());
    let response = response.on_hover_text(match button {
        LinuxTitleBarButton::Minimize => "Minimize",
        LinuxTitleBarButton::Maximize => "Maximize",
        LinuxTitleBarButton::Restore => "Restore",
        LinuxTitleBarButton::Close => "Close",
    });
    if response.hovered() {
        let fill = if matches!(button, LinuxTitleBarButton::Close) {
            egui::Color32::from_rgb(196, 43, 28)
        } else {
            ui.visuals().widgets.hovered.weak_bg_fill
        };
        ui.painter().rect_filled(rect, 0.0, fill);
    }

    let color = if response.hovered() && matches!(button, LinuxTitleBarButton::Close) {
        egui::Color32::WHITE
    } else {
        ui.visuals().text_color()
    };
    let stroke = egui::Stroke::new(1.25, color);
    let center = rect.center();
    match button {
        LinuxTitleBarButton::Minimize => {
            ui.painter().line_segment(
                [
                    center + egui::vec2(-5.0, 4.0),
                    center + egui::vec2(5.0, 4.0),
                ],
                stroke,
            );
        }
        LinuxTitleBarButton::Maximize => {
            let min = center + egui::vec2(-5.0, -5.0);
            let max = center + egui::vec2(5.0, 5.0);
            ui.painter()
                .line_segment([min, egui::pos2(max.x, min.y)], stroke);
            ui.painter()
                .line_segment([egui::pos2(max.x, min.y), max], stroke);
            ui.painter()
                .line_segment([max, egui::pos2(min.x, max.y)], stroke);
            ui.painter()
                .line_segment([egui::pos2(min.x, max.y), min], stroke);
        }
        LinuxTitleBarButton::Restore => {
            let back_min = center + egui::vec2(-3.0, -6.0);
            let back_max = center + egui::vec2(6.0, 3.0);
            ui.painter()
                .line_segment([back_min, egui::pos2(back_max.x, back_min.y)], stroke);
            ui.painter()
                .line_segment([egui::pos2(back_max.x, back_min.y), back_max], stroke);
            let front_min = center + egui::vec2(-6.0, -3.0);
            let front_max = center + egui::vec2(3.0, 6.0);
            ui.painter()
                .line_segment([front_min, egui::pos2(front_max.x, front_min.y)], stroke);
            ui.painter()
                .line_segment([egui::pos2(front_max.x, front_min.y), front_max], stroke);
            ui.painter()
                .line_segment([front_max, egui::pos2(front_min.x, front_max.y)], stroke);
            ui.painter()
                .line_segment([egui::pos2(front_min.x, front_max.y), front_min], stroke);
        }
        LinuxTitleBarButton::Close => {
            ui.painter().line_segment(
                [
                    center + egui::vec2(-5.0, -5.0),
                    center + egui::vec2(5.0, 5.0),
                ],
                stroke,
            );
            ui.painter().line_segment(
                [
                    center + egui::vec2(-5.0, 5.0),
                    center + egui::vec2(5.0, -5.0),
                ],
                stroke,
            );
        }
    }
    response
}

#[cfg(target_os = "linux")]
pub(super) fn draw_linux_title_bar(ctx: &egui::Context, logo: &egui::TextureHandle) -> bool {
    let mut close_clicked = false;
    egui::TopBottomPanel::top("linux_title_bar")
        .exact_height(36.0)
        .frame(
            egui::Frame::new()
                .fill(ctx.style().visuals.window_fill)
                .inner_margin(0.0),
        )
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            let response = ui.interact(
                rect,
                egui::Id::new("linux_title_bar_drag"),
                egui::Sense::click_and_drag(),
            );
            if response.double_clicked() {
                let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            if response.drag_started_by(egui::PointerButton::Primary) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            ui.painter().image(
                logo.id(),
                egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 21.0, rect.center().y),
                    egui::vec2(24.0, 24.0),
                ),
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            ui.painter().text(
                egui::pos2(rect.left() + 41.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                "Sundial",
                egui::FontId::proportional(14.0),
                ui.visuals().text_color(),
            );
            ui.painter().line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                ui.visuals().widgets.noninteractive.bg_stroke,
            );

            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    close_clicked =
                        linux_title_bar_button(ui, LinuxTitleBarButton::Close).clicked();
                    let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
                    let maximize_button = if maximized {
                        LinuxTitleBarButton::Restore
                    } else {
                        LinuxTitleBarButton::Maximize
                    };
                    if linux_title_bar_button(ui, maximize_button).clicked() {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                    }
                    if linux_title_bar_button(ui, LinuxTitleBarButton::Minimize).clicked() {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                },
            );
        });
    close_clicked
}

pub(super) fn open_directory(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("Could not create {}: {error}", path.display()))?;
    let mut command = if cfg!(target_os = "windows") {
        Command::new("explorer.exe")
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    command
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open {}: {error}", path.display()))
}

#[cfg(windows)]
pub(super) fn destiny_is_running() -> Result<bool, String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };

    // SAFETY: This snapshot mode takes no caller-owned pointers; the returned handle is checked
    // against INVALID_HANDLE_VALUE and closed exactly once below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(format!(
            "Could not check whether Destiny 2 is running: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: PROCESSENTRY32W is a Windows C data structure for which all-zero is a valid initial
    // state. Its required dwSize field is initialized before the structure is passed to Windows.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = u32::try_from(std::mem::size_of::<PROCESSENTRY32W>())
        .expect("PROCESSENTRY32W size fits in u32");
    let mut found = false;
    // SAFETY: snapshot is a live ToolHelp handle and entry points to writable storage with dwSize
    // initialized to the exact PROCESSENTRY32W size.
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while has_entry {
        let length = entry
            .szExeFile
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(entry.szExeFile.len());
        let executable = String::from_utf16_lossy(&entry.szExeFile[..length]);
        if executable.eq_ignore_ascii_case("destiny2.exe") {
            found = true;
            break;
        }
        // SAFETY: The same live snapshot handle and initialized writable entry remain valid for
        // the duration of the enumeration.
        has_entry = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    // SAFETY: snapshot was returned successfully above and has not been closed or transferred.
    unsafe {
        CloseHandle(snapshot);
    }
    Ok(found)
}

#[cfg(target_os = "linux")]
pub(super) fn destiny_is_running() -> Result<bool, String> {
    let processes = fs::read_dir("/proc")
        .map_err(|error| format!("Could not check whether Destiny 2 is running: {error}"))?;
    for process in processes.flatten() {
        if !process
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        if fs::read_to_string(process.path().join("comm"))
            .is_ok_and(|name| name.trim().eq_ignore_ascii_case("destiny2.exe"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(super) fn destiny_is_running() -> Result<bool, String> {
    let output = Command::new("pgrep")
        .args(["-ix", "destiny2.exe"])
        .output()
        .map_err(|error| format!("Could not check whether Destiny 2 is running: {error}"))?;
    Ok(output.status.success())
}

#[cfg(windows)]
pub(super) fn set_windows_app_identity() {
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    let app_id = "KyleThompson.Sundial\0".encode_utf16().collect::<Vec<_>>();
    // SAFETY: app_id is NUL-terminated and remains alive for the duration of the synchronous call.
    let _ = unsafe { SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr()) };
}

#[cfg(windows)]
pub(super) fn set_windows_taskbar_icon(context: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::{
        Foundation::HWND,
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            GCLP_HICON, GCLP_HICONSM, GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON,
            LR_SHARED, LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW,
            SetClassLongPtrW, WM_SETICON,
        },
    };

    let Ok(window_handle) = context.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(window_handle) = window_handle.as_raw() else {
        return;
    };
    let window = window_handle.hwnd.get() as HWND;

    // build.rs embeds the ICO as resource 1. Shared resource handles remain valid
    // for the process lifetime and do not need application-side destruction.
    // SAFETY: A null module-name pointer requests the module for the current executable.
    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    for (kind, class_index, width_metric, height_metric) in [
        (ICON_BIG, GCLP_HICON, SM_CXICON, SM_CYICON),
        (ICON_SMALL, GCLP_HICONSM, SM_CXSMICON, SM_CYSMICON),
    ] {
        // SAFETY: These calls take validated Windows metric constants and no caller-owned pointers.
        let (width, height) = unsafe {
            (
                GetSystemMetrics(width_metric),
                GetSystemMetrics(height_metric),
            )
        };
        // SAFETY: module is the current executable module, resource ID 1 is encoded with the
        // MAKEINTRESOURCEW pointer convention, and all remaining arguments are valid icon values.
        let icon = unsafe {
            LoadImageW(
                module,
                std::ptr::without_provenance(1),
                IMAGE_ICON,
                width,
                height,
                LR_SHARED,
            )
        };
        if icon.is_null() {
            continue;
        }

        // SAFETY: window comes from eframe's live Win32 window handle, and icon is a successful
        // LR_SHARED resource handle that remains valid for the lifetime of the process.
        unsafe {
            SendMessageW(window, WM_SETICON, kind as usize, icon as isize);
            SetClassLongPtrW(window, class_index, icon as isize);
        }
    }
}
