//! Windows toast notifications for due reminders.

use std::path::Path;
use tauri_winrt_notification::Toast;

const APP_ID: &str = "QuickPlanner";

/// What the user clicked on a toast.
pub enum Click {
    Open,
    Done,
}

/// Registers the name and icon toasts show, under HKCU so no installer or admin rights are
/// needed. The icon is written as a PNG into `dir`.
pub fn register(dir: &Path) {
    let icon = dir.join("icon.png");
    std::fs::create_dir_all(dir).ok();
    if let Err(err) = write_icon(&icon) {
        eprintln!("could not write notification icon: {err}");
    }
    set_value("DisplayName", "Quick Planner");
    set_value("IconUri", &icon.to_string_lossy());
}

fn write_icon(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut png = png::Encoder::new(file, crate::icon::SIZE, crate::icon::SIZE);
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    png.write_header()?.write_image_data(&crate::icon::rgba())?;
    Ok(())
}

fn set_value(name: &str, value: &str) {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (key, name, value) = (wide(&format!(r"Software\Classes\AppUserModelId\{APP_ID}")), wide(name), wide(value));
    // SAFETY: NUL-terminated UTF-16 strings; the size is in bytes and includes the NUL.
    let err = unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, value.as_ptr().cast(), value.len() as u32 * 2)
    };
    if err != 0 {
        eprintln!("could not register notifications: error {err}");
    }
}

/// Shows a toast, with a "Mark done" button when `done` is set. `on_click` runs on a
/// system thread, for a click on the toast or its button while the app is running.
pub fn show(title: &str, body: &str, note: &str, done: bool, on_click: impl Fn(Click) + Send + 'static) {
    let mut toast = Toast::new(APP_ID).title(title).text1(body);
    if !note.is_empty() {
        toast = toast.text2(note);
    }
    if done {
        toast = toast.add_button("Mark done", "done");
    }
    let shown = toast
        .on_activated(move |action| {
            on_click(if action.as_deref() == Some("done") { Click::Done } else { Click::Open });
            Ok(())
        })
        .show();
    if let Err(err) = shown {
        eprintln!("could not show reminder: {err}");
    }
}
