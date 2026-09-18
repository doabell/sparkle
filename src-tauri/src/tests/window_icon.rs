use super::*;

#[test]
fn accent_icon_recolors_the_logo_without_changing_its_transparency() {
    let original = image::load_from_memory(include_bytes!("../../icons/128x128@2x.png"))
        .unwrap()
        .into_rgba8();
    let icon = accent_icon(" #12ABef ").unwrap();
    assert_eq!((icon.width(), icon.height()), original.dimensions());
    for (pixel, original) in icon.rgba().chunks_exact(4).zip(original.pixels()) {
        assert_eq!(&pixel[..3], &[0x12, 0xab, 0xef]);
        assert_eq!(pixel[3], original.0[3]);
    }
    assert!(icon.rgba().chunks_exact(4).any(|p| p[3] == 0));
    assert!(icon.rgba().chunks_exact(4).any(|p| p[3] == 255));
}

#[test]
fn invalid_accent_uses_the_default_logo_color() {
    let icon = accent_icon("invalid").unwrap();
    assert_eq!(&icon.rgba()[..3], &[0xfa, 0x24, 0x3c]);
}

#[cfg(windows)]
#[test]
fn taskbar_icon_tracks_the_small_icon_and_survives_replacement() {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    // A hidden, test-owned native window exercises WM_GETICON/WM_SETICON
    // without launching the player or touching the user's library.
    unsafe {
        let hwnd = CreateWindowExW(
            0,
            windows_sys::w!("STATIC"),
            windows_sys::w!("Sparkle icon test"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        assert!(!hwnd.is_null());
        struct TestWindow(windows_sys::Win32::Foundation::HWND);
        impl Drop for TestWindow {
            fn drop(&mut self) {
                unsafe {
                    DestroyWindow(self.0);
                }
            }
        }
        let _window = TestWindow(hwnd);
        assert!(taskbar::copy_window_icon(hwnd).is_err());
        for resource in [IDI_APPLICATION, IDI_INFORMATION] {
            let small = LoadIconW(std::ptr::null_mut(), resource);
            assert!(!small.is_null());
            SendMessageW(hwnd, WM_SETICON, ICON_SMALL as usize, small as isize);
            taskbar::copy_window_icon(hwnd).unwrap();
            let large = SendMessageW(hwnd, WM_GETICON, ICON_BIG as usize, 0);
            assert_ne!(large, 0);
            assert_ne!(
                large, small as isize,
                "the taskbar owns an independent copy"
            );
            assert_eq!(
                SendMessageW(hwnd, WM_GETICON, ICON_SMALL as usize, 0),
                small as isize
            );
        }
    }
}
