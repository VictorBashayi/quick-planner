#![windows_subsystem = "windows"]

mod icon;
mod store;

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use slint::winit_030::{WinitWindowAccessor, winit};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use store::{Bucket, State};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

slint::include_modules!();

struct App {
    state: RefCell<State>,
    path: PathBuf,
    save_timer: Timer,
    window: slint::Weak<AppWindow>,
    widget: slint::Weak<WidgetWindow>,
    visible: Cell<bool>,
    placed: Cell<bool>,
}

impl App {
    fn ui(&self) -> AppWindow {
        self.window.upgrade().expect("window outlives app state")
    }

    /// Pushes state into the UI and schedules a save.
    fn changed(self: &Rc<Self>) {
        self.state.borrow_mut().tidy();
        self.refresh();
        self.schedule_save();
    }

    fn schedule_save(self: &Rc<Self>) {
        let app = Rc::downgrade(self);
        self.save_timer.start(TimerMode::SingleShot, Duration::from_millis(300), move || {
            if let Some(app) = app.upgrade() {
                app.flush();
            }
        });
    }

    fn flush(&self) {
        self.save_timer.stop();
        if let Err(err) = store::save(&self.path, &self.state.borrow()) {
            eprintln!("could not save tasks: {err}");
        }
    }

    fn refresh(&self) {
        let ui = self.ui();
        let state = self.state.borrow();
        let now = store::now_ms();
        let days = local_day(now).zip(local_day(now - 86_400_000));
        let buckets: Vec<BucketItem> = Bucket::ALL
            .into_iter()
            .map(|b| {
                let tasks: Vec<TaskItem> = state
                    .tasks_in(b)
                    .map(|t| TaskItem {
                        id: t.id as i32,
                        title: t.text.as_str().into(),
                        desc: t.desc.as_str().into(),
                        done: t.completed,
                        when: match (t.completed_at.and_then(local_day), days) {
                            (Some(d), Some((today, yesterday))) if t.completed => {
                                completed_label(d, today, yesterday).into()
                            }
                            _ => SharedString::new(),
                        },
                    })
                    .collect();
                BucketItem {
                    key: b.key().into(),
                    title: bucket_title(b).into(),
                    collapsed: state.is_collapsed(b),
                    empty_text: empty_text(b).into(),
                    tasks: ModelRc::new(VecModel::from(tasks)),
                }
            })
            .collect();
        ui.set_buckets(ModelRc::new(VecModel::from(buckets)));
        ui.set_selected(match state.selected_bucket {
            Bucket::Soon => 1,
            Bucket::Later => 2,
            _ => 0,
        });
        ui.set_status(status_text(state.counts()).into());
        ui.set_has_done(state.counts().1 > 0);

        // The widget shows today's open tasks only, capped so it never needs scrolling.
        const WIDGET_MAX: usize = 6;
        let open: Vec<_> = state.tasks_in(Bucket::Now).filter(|t| !t.completed).collect();
        let w = self.widget.upgrade().expect("widget outlives app state");
        w.set_tasks(ModelRc::new(VecModel::from(
            open.iter()
                .take(WIDGET_MAX)
                .map(|t| TaskItem { id: t.id as i32, title: t.text.as_str().into(), ..Default::default() })
                .collect::<Vec<_>>(),
        )));
        w.set_more(match open.len().saturating_sub(WIDGET_MAX) {
            0 => SharedString::new(),
            n => format!("+{n} more").into(),
        });
        w.set_today(today().into());
    }

    fn show(&self) {
        let ui = self.ui();
        ui.set_today(today().into());
        // Completion labels say "today"/"yesterday", so refresh them across midnight.
        self.refresh();
        if ui.show().is_err() {
            return;
        }
        self.visible.set(true);
        ui.window().with_winit_window(|w| {
            if !self.placed.replace(true) {
                center(w);
            }
            w.focus_window();
        });
        ui.invoke_focus_input();
    }

    fn hide(&self) {
        self.visible.set(false);
        self.ui().hide().ok();
    }

    fn toggle(&self) {
        if self.visible.get() { self.hide() } else { self.show() }
    }

    /// Stores the widget's place when it has been moved or resized.
    fn remember_widget(self: &Rc<Self>) {
        let Some(widget) = self.widget.upgrade() else { return };
        if !widget.window().is_visible() {
            return;
        }
        let place = widget
            .window()
            .with_winit_window(|w| {
                let (p, s) = (w.outer_position().ok()?, w.inner_size());
                Some([p.x, p.y, s.width as i32, s.height as i32])
            })
            .flatten();
        if place.is_some() && place != self.state.borrow().widget {
            self.state.borrow_mut().widget = place;
            self.schedule_save();
        }
    }
}

fn bucket_title(b: Bucket) -> &'static str {
    match b {
        Bucket::Now => "Today",
        Bucket::Soon => "This week",
        Bucket::Later => "Someday",
        Bucket::Done => "Done",
    }
}

fn empty_text(b: Bucket) -> &'static str {
    match b {
        Bucket::Now => "Nothing for today yet.",
        Bucket::Soon => "Nothing planned this week.",
        Bucket::Later => "Ideas for someday go here.",
        Bucket::Done => "Finished tasks land here.",
    }
}

fn status_text((open, done): (usize, usize)) -> String {
    match (open, done) {
        (0, 0) => "No tasks yet".into(),
        (o, 0) => format!("{o} open"),
        (0, d) => format!("All done, {d} finished"),
        (o, d) => format!("{o} open, {d} done"),
    }
}

const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
    "November", "December",
];

#[derive(Clone, Copy, Debug, PartialEq)]
struct Day {
    year: u16,
    month: u16,
    day: u16,
    weekday: u16,
}

/// The local calendar day of a Unix timestamp in milliseconds.
fn local_day(ms: i64) -> Option<Day> {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let ticks = ms.checked_add(11_644_473_600_000)?.checked_mul(10_000)? as u64;
    let ft = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    // SAFETY: both calls only read and write the structs they are given.
    let t = unsafe {
        let mut utc: SYSTEMTIME = std::mem::zeroed();
        let mut local: SYSTEMTIME = std::mem::zeroed();
        if FileTimeToSystemTime(&ft, &mut utc) == 0
            || SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) == 0
        {
            return None;
        }
        local
    };
    Some(Day { year: t.wYear, month: t.wMonth, day: t.wDay, weekday: t.wDayOfWeek })
}

/// "Completed today", "Completed yesterday" or "Completed Mon 20 Oct", with the year
/// only when it isn't this year.
fn completed_label(done: Day, today: Day, yesterday: Day) -> String {
    if done == today {
        return "Completed today".into();
    }
    if done == yesterday {
        return "Completed yesterday".into();
    }
    let date = format!(
        "{} {} {}",
        &DAYS[done.weekday as usize % 7][..3],
        done.day,
        &MONTHS[(done.month as usize).saturating_sub(1) % 12][..3]
    );
    if done.year == today.year { format!("Completed {date}") } else { format!("Completed {date} {}", done.year) }
}

fn today() -> String {
    use windows_sys::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: GetLocalTime only writes the struct it is given.
    let t = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    format!(
        "{}, {} {}",
        DAYS[t.wDayOfWeek as usize % 7],
        t.wDay,
        MONTHS[(t.wMonth as usize).saturating_sub(1) % 12]
    )
}

fn hwnd(w: &winit::window::Window) -> Option<windows_sys::Win32::Foundation::HWND> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match w.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as _),
        _ => None,
    }
}

/// Makes the desktop the widget's owner so Win+D leaves it in place, and hides it from
/// screen capture so sampling the backdrop doesn't see the widget's own text.
fn pin_to_desktop(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let class: Vec<u16> = "Progman\0".encode_utf16().collect();
    // SAFETY: plain Win32 calls on a window this process owns; a null Progman is skipped.
    unsafe {
        let progman = FindWindowW(class.as_ptr(), std::ptr::null());
        if !progman.is_null() {
            SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, progman as isize);
        }
        SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
    }
}

/// Average brightness (0..1) of what is on screen behind the widget, or `None` when
/// another window covers it and the reading would not be the wallpaper.
// shortcut: only the centre point is checked for cover; check corners if half-covered widgets misjudge.
fn backdrop_luma(hwnd: windows_sys::Win32::Foundation::HWND) -> Option<f32> {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowRect, WindowFromPoint};
    const N: i32 = 8;
    // SAFETY: every GDI object created here is released before returning, and the DIB
    // pixel pointer is only read while its bitmap is alive.
    unsafe {
        let mut r: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut r) == 0 {
            return None;
        }
        if WindowFromPoint(POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 }) != hwnd {
            return None;
        }
        let screen = GetDC(std::ptr::null_mut());
        let mem = CreateCompatibleDC(screen);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = N;
        info.bmiHeader.biHeight = -N;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(mem, &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        let mut luma = None;
        if !bmp.is_null() {
            let old = SelectObject(mem, bmp);
            SetStretchBltMode(mem, HALFTONE);
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            if StretchBlt(mem, 0, 0, N, N, screen, r.left, r.top, w, h, SRCCOPY) != 0 {
                luma = Some(mean_luma(std::slice::from_raw_parts(bits as *const u32, (N * N) as usize)));
            }
            SelectObject(mem, old);
            DeleteObject(bmp);
        }
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);
        luma
    }
}

/// Mean perceived brightness of 0x00RRGGBB pixels, 0 (black) to 1 (white).
fn mean_luma(px: &[u32]) -> f32 {
    let sum: f32 = px
        .iter()
        .map(|p| {
            let (r, g, b) = ((p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff);
            0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32
        })
        .sum();
    sum / (px.len().max(1) as f32 * 255.0)
}

/// Whether the backdrop counts as dark. The gap between thresholds stops the text
/// flipping back and forth on mid-grey wallpapers.
fn is_dark(luma: f32, was_dark: bool) -> bool {
    if was_dark { luma < 0.55 } else { luma < 0.45 }
}

/// Whether a physical screen point lies on any connected monitor.
fn on_screen(w: &winit::window::Window, x: i32, y: i32) -> bool {
    w.available_monitors().any(|m| {
        let (p, s) = (m.position(), m.size());
        x >= p.x && y >= p.y && x < p.x + s.width as i32 && y < p.y + s.height as i32
    })
}

fn center(w: &winit::window::Window) {
    let Some(monitor) = w.current_monitor().or_else(|| w.primary_monitor()) else { return };
    let (m, ms, ws) = (monitor.position(), monitor.size(), w.outer_size());
    let x = m.x + (ms.width as i32 - ws.width as i32) / 2;
    let y = m.y + ((ms.height as i32 - ws.height as i32) / 2 - 40).max(40);
    w.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
}

/// Holds a named mutex for the life of the process; `None` if another copy runs.
fn single_instance() -> Option<isize> {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    let name: Vec<u16> = "Local\\QuickPlannerSingleInstance\0".encode_utf16().collect();
    // SAFETY: valid NUL-terminated name, default security attributes.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    (!handle.is_null() && !exists).then_some(handle as isize)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Some(_instance) = single_instance() else { return Ok(()) };

    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(|attrs| {
            use winit::platform::windows::{CornerPreference, WindowAttributesExtWindows};
            let icon = winit::window::Icon::from_rgba(icon::rgba(), icon::SIZE, icon::SIZE).ok();
            attrs
                .with_window_icon(icon)
                .with_skip_taskbar(true)
                .with_undecorated_shadow(true)
                .with_corner_preference(CornerPreference::Round)
                .with_drag_and_drop(false)
        })
        .select()?;

    let ui = AppWindow::new()?;
    let widget = WidgetWindow::new()?;
    let path = store::data_path();
    let app = Rc::new(App {
        state: RefCell::new(store::load(&path)),
        path,
        save_timer: Timer::default(),
        window: ui.as_weak(),
        widget: widget.as_weak(),
        visible: Cell::new(false),
        placed: Cell::new(false),
    });
    app.refresh();

    let on = |f: fn(&Rc<App>)| {
        let app = app.clone();
        move || f(&app)
    };
    {
        let a = app.clone();
        ui.on_add_task(move |text| {
            let b = a.state.borrow().selected_bucket;
            a.state.borrow_mut().add(&text, b);
            a.changed();
        });
    }
    {
        let a = app.clone();
        ui.on_select_bucket(move |i| {
            a.state.borrow_mut().selected_bucket = [Bucket::Now, Bucket::Soon, Bucket::Later][i.clamp(0, 2) as usize];
            a.refresh();
            a.schedule_save();
        });
    }
    {
        let a = app.clone();
        ui.on_toggle_task(move |id| {
            a.state.borrow_mut().toggle(id as u64);
            a.changed();
        });
    }
    {
        let a = app.clone();
        ui.on_delete_task(move |id| {
            a.state.borrow_mut().delete(id as u64);
            a.changed();
        });
    }
    {
        let a = app.clone();
        ui.on_move_task(move |id, key, index| {
            if let Some(b) = Bucket::from_key(&key) {
                a.state.borrow_mut().move_to(id as u64, b, index.max(0) as usize);
                a.changed();
            }
        });
    }
    {
        // Live edits update state without rebuilding the list, so the editor keeps focus.
        let a = app.clone();
        ui.on_edit_live(move |id, is_desc, text| {
            let mut s = a.state.borrow_mut();
            if is_desc { s.set_desc(id as u64, &text) } else { s.set_text(id as u64, &text) }
            drop(s);
            a.schedule_save();
        });
    }
    ui.on_edit_finished(on(|a| a.changed()));
    {
        let a = app.clone();
        ui.on_edit_cancelled(move |id, title: SharedString, desc: SharedString| {
            let mut s = a.state.borrow_mut();
            s.set_text(id as u64, &title);
            s.set_desc(id as u64, &desc);
            drop(s);
            a.changed();
        });
    }
    {
        let a = app.clone();
        ui.on_toggle_collapsed(move |key| {
            if let Some(b) = Bucket::from_key(&key) {
                a.state.borrow_mut().toggle_collapsed(b);
                a.changed();
            }
        });
    }
    ui.on_clear_done(on(|a| {
        a.state.borrow_mut().clear_done();
        a.changed();
    }));
    ui.on_show_done(on(|a| {
        a.state.borrow_mut().expand(Bucket::Done);
        a.changed();
    }));
    ui.on_hide_window(on(|a| a.hide()));
    {
        let w = ui.as_weak();
        ui.on_start_move(move || {
            if let Some(ui) = w.upgrade() {
                ui.window().with_winit_window(|w| w.drag_window().ok());
            }
        });
    }
    {
        let w = ui.as_weak();
        ui.on_start_resize(move || {
            if let Some(ui) = w.upgrade() {
                ui.window().with_winit_window(|w| {
                    w.drag_resize_window(winit::window::ResizeDirection::SouthEast).ok()
                });
            }
        });
    }
    {
        let a = app.clone();
        widget.on_toggle_task(move |id| {
            a.state.borrow_mut().toggle(id as u64);
            a.changed();
        });
    }
    widget.on_open_app(on(|a| a.show()));
    {
        let w = widget.as_weak();
        widget.on_start_move(move || {
            if let Some(w) = w.upgrade() {
                w.window().with_winit_window(|w| w.drag_window().ok());
            }
        });
    }
    {
        let w = widget.as_weak();
        widget.on_start_resize(move || {
            if let Some(w) = w.upgrade() {
                w.window().with_winit_window(|w| {
                    w.drag_resize_window(winit::window::ResizeDirection::SouthEast).ok()
                });
            }
        });
    }
    // Clicking the window's close affordances (Alt+F4) hides instead of quitting.
    {
        let a = app.clone();
        ui.window().on_close_requested(move || {
            a.hide();
            slint::CloseRequestResponse::KeepWindowShown
        });
    }

    // Tray icon and menu.
    let open_item = MenuItem::new("Open Quick Planner", true, None);
    let widget_item = MenuItem::new("Show or hide widget", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append_items(&[&open_item, &widget_item, &PredefinedMenuItem::separator(), &quit_item])?;
    let _tray = TrayIconBuilder::new()
        .with_icon(tray_icon::Icon::from_rgba(icon::rgba(), icon::SIZE, icon::SIZE)?)
        .with_tooltip("Quick Planner (Alt+P)")
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()?;

    // Global shortcut. If another app already owns Alt+P the tray still works.
    let hotkeys = GlobalHotKeyManager::new()?;
    let hotkey = HotKey::new(Some(Modifiers::ALT), Code::KeyP);
    let _ = hotkeys.register(hotkey);

    // These handlers run on the event loop's own thread; they forward to the
    // UI through the event loop queue, so nothing polls while idle.
    let w = ui.as_weak();
    GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
        if e.id == hotkey.id() && e.state == HotKeyState::Pressed {
            w.upgrade_in_event_loop(|ui| ui.invoke_toggle_requested()).ok();
        }
    }));
    let w = ui.as_weak();
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
            w.upgrade_in_event_loop(|ui| ui.invoke_toggle_requested()).ok();
        }
    }));
    let w = ui.as_weak();
    let (open_id, quit_id, widget_id) = (open_item.id().clone(), quit_item.id().clone(), widget_item.id().clone());
    let wv = widget.as_weak();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id == open_id {
            w.upgrade_in_event_loop(|ui| ui.invoke_show_requested()).ok();
        } else if e.id == widget_id {
            let wv = wv.clone();
            slint::invoke_from_event_loop(move || {
                if let Some(w) = wv.upgrade() {
                    if w.window().is_visible() { w.hide().ok(); } else { w.show().ok(); }
                }
            })
            .ok();
        } else if e.id == quit_id {
            w.upgrade_in_event_loop(|ui| ui.invoke_quit_requested()).ok();
        }
    }));
    ui.on_toggle_requested(on(|a| a.toggle()));
    ui.on_show_requested(on(|a| a.show()));
    ui.on_quit_requested(on(|a| {
        a.remember_widget();
        a.flush();
        slint::quit_event_loop().ok();
    }));

    // The widget goes back where it was left, if that spot is still on a connected
    // monitor; otherwise top-right of the primary monitor. Slint only creates the native
    // window once the event loop runs, so placing and pinning wait for it.
    widget.show()?;
    let saved = app.state.borrow().widget;
    let ww = widget.as_weak();
    slint::spawn_local(async move {
        let Some(widget) = ww.upgrade() else { return };
        let Ok(w) = widget.window().winit_window().await else { return };
        if let Some([x, y, width, height]) = saved.filter(|&[x, y, ..]| on_screen(&w, x + 40, y + 10)) {
            let _ = w.request_inner_size(winit::dpi::PhysicalSize::new(width as u32, height as u32));
            w.set_outer_position(winit::dpi::PhysicalPosition::new(x, y));
        } else if let Some(m) = w.primary_monitor() {
            let x = m.position().x + m.size().width as i32 - w.outer_size().width as i32 - 24;
            w.set_outer_position(winit::dpi::PhysicalPosition::new(x, m.position().y + 24));
        }
        hwnd(&w).map(pin_to_desktop);
    })?;

    // Re-check the wallpaper behind the widget now and then; it may move or the wallpaper change.
    let backdrop_timer = Timer::default();
    // The same tick saves the widget's place, so moves and resizes need no event hooks.
    let check_backdrop = {
        let w = widget.as_weak();
        let a = app.clone();
        move || {
            a.remember_widget();
            let Some(widget) = w.upgrade() else { return };
            if !widget.window().is_visible() {
                return;
            }
            if let Some(luma) = widget.window().with_winit_window(|w| hwnd(w).and_then(backdrop_luma)).flatten() {
                widget.set_dark_bg(is_dark(luma, widget.get_dark_bg()));
            }
        }
    };
    check_backdrop();
    backdrop_timer.start(TimerMode::Repeated, Duration::from_secs(5), check_backdrop);

    app.show();
    slint::run_event_loop_until_quit()?;
    app.flush();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(year: u16, month: u16, day: u16, weekday: u16) -> Day {
        Day { year, month, day, weekday }
    }

    #[test]
    fn completed_label_names_recent_days_and_dates_older_ones() {
        let today = day(2026, 10, 26, 1);
        let yesterday = day(2026, 10, 25, 0);
        assert_eq!(completed_label(today, today, yesterday), "Completed today");
        assert_eq!(completed_label(yesterday, today, yesterday), "Completed yesterday");
        assert_eq!(completed_label(day(2026, 10, 20, 2), today, yesterday), "Completed Tue 20 Oct");
        assert_eq!(completed_label(day(2025, 12, 31, 3), today, yesterday), "Completed Wed 31 Dec 2025");
    }

    #[test]
    fn backdrop_brightness_picks_text_tone() {
        assert_eq!(mean_luma(&[0x000000, 0xffffff]), 0.5);
        assert!(mean_luma(&[0x0000ff]) < 0.1, "pure blue reads as dark");
        assert!(is_dark(0.2, false) && !is_dark(0.8, true));
        // Between the thresholds the current tone is kept.
        assert!(is_dark(0.5, true) && !is_dark(0.5, false));
    }

    #[test]
    fn local_day_reads_a_timestamp() {
        // Noon UTC stays on the same calendar day in every time zone from UTC-11 to UTC+11.
        let d = local_day(1_793_016_000_000).unwrap(); // 2026-10-26 12:00 UTC
        assert_eq!((d.year, d.month, d.day), (2026, 10, 26));
    }
}
