#[tauri::command]
pub(crate) fn set_window_transitions_enabled(
    window: tauri::WebviewWindow,
    enabled: bool,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED,
        };

        let hwnd = window.hwnd().map_err(|error| error.to_string())?;
        let disabled = i32::from(!enabled);
        // The handle belongs to this window and DWM reads the BOOL during the call.
        let result = unsafe {
            DwmSetWindowAttribute(
                hwnd.0,
                DWMWA_TRANSITIONS_FORCEDISABLED as u32,
                std::ptr::from_ref(&disabled).cast(),
                std::mem::size_of_val(&disabled) as u32,
            )
        };
        if result < 0 {
            return Err(format!("DwmSetWindowAttribute failed ({result:#010x})"));
        }
    }
    #[cfg(not(windows))]
    let _ = (window, enabled);

    Ok(())
}
