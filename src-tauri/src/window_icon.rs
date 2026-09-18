use tauri::{image::Image, AppHandle, Manager};

fn accent_icon(color: &str) -> Result<Image<'static>, String> {
    let color = crate::settings::normalize_accent_color(color);
    let channel = |start| u8::from_str_radix(&color[start..start + 2], 16).unwrap();
    let rgb = [channel(1), channel(3), channel(5)];
    let mut icon = image::load_from_memory(include_bytes!("../icons/128x128@2x.png"))
        .map_err(|e| e.to_string())?
        .into_rgba8();
    // Preserve the bundled logo's shape and antialiasing; only its color changes.
    for pixel in icon.pixels_mut() {
        pixel.0[..3].copy_from_slice(&rgb);
    }
    let (width, height) = icon.dimensions();
    Ok(Image::new_owned(icon.into_raw(), width, height))
}

pub fn apply_accent(app: &AppHandle, color: &str) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let result = accent_icon(color).and_then(|icon| {
        let target = window.clone();
        window
            .run_on_main_thread(move || {
                let result = set_window_icon(&target, icon);
                log_icon_error(result);
            })
            .map_err(|e| e.to_string())
    });
    log_icon_error(result);
}

fn log_icon_error(result: Result<(), String>) {
    if let Err(error) = result {
        log::warn!(target: "sparkle::window", "event=icon_update_failed error={error}");
    }
}

fn set_window_icon(window: &tauri::WebviewWindow, icon: Image<'_>) -> Result<(), String> {
    window.set_icon(icon).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?;
        // This runs on the window thread, after Tauri has set ICON_SMALL.
        unsafe {
            taskbar::copy_window_icon(hwnd.0)?;
        }
    }
    Ok(())
}

#[cfg(windows)]
mod taskbar {
    use std::cell::RefCell;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CopyIcon, DestroyIcon, SendMessageW, HICON, ICON_BIG, ICON_SMALL, WM_GETICON, WM_SETICON,
    };

    struct OwnedIcon(HICON);

    impl Drop for OwnedIcon {
        fn drop(&mut self) {
            // Only the CopyIcon handle belongs to us; Tauri owns ICON_SMALL.
            unsafe {
                DestroyIcon(self.0);
            }
        }
    }

    thread_local! {
        // Sparkle has one main window. Keep its large icon alive on the UI
        // thread until replacement or shutdown, without leaking each color.
        static ICON: RefCell<Option<OwnedIcon>> = const { RefCell::new(None) };
    }

    /// The caller must own the window and run on its event-loop thread.
    pub(super) unsafe fn copy_window_icon(hwnd: HWND) -> Result<(), String> {
        let small = SendMessageW(hwnd, WM_GETICON, ICON_SMALL as usize, 0) as HICON;
        if small.is_null() {
            return Err("window icon is unavailable".into());
        }
        let large = CopyIcon(small);
        if large.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // Tauri/Tao currently sets only the small caption icon. Windows uses
        // the large icon for the taskbar and Alt+Tab, so set it explicitly.
        SendMessageW(hwnd, WM_SETICON, ICON_BIG as usize, large as isize);
        ICON.with(|icon| {
            icon.replace(Some(OwnedIcon(large)));
        });
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/window_icon.rs"]
mod tests;
