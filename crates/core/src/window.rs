use std::time::Duration;

#[cfg(windows)]
pub fn run_game_window(title: &str, duration: Duration) -> bool {
    imp::run(title, duration)
}

#[cfg(not(windows))]
pub fn run_game_window(_title: &str, _duration: Duration) -> bool {
    false
}

#[cfg(windows)]
mod imp {
    use crate::timer::format_clock;
    use std::cell::RefCell;
    use std::ptr::{null, null_mut};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreateSolidBrush,
        DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, InvalidateRect, SelectObject, SetBkMode,
        SetTextColor, UpdateWindow, CLEARTYPE_QUALITY, DEFAULT_CHARSET, DT_CENTER, DT_END_ELLIPSIS,
        DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, HDC, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRect, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, LoadCursorW,
        MsgWaitForMultipleObjects, PeekMessageW, PostQuitMessage, RegisterClassW, SetProcessDPIAware, SetTimer,
        ShowWindow, TranslateMessage, CW_USEDEFAULT, IDC_ARROW, MSG, PM_REMOVE, QS_ALLINPUT, SW_SHOWNOACTIVATE,
        WM_DESTROY, WM_ERASEBKGND, WM_PAINT, WM_QUIT, WM_TIMER, WNDCLASSW, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED,
        WS_SYSMENU,
    };

    const BG: u32 = 0x191919;
    const INK: u32 = 0xf2f2f2;
    const MUTED: u32 = 0xababab;

    const CLIENT_W: f32 = 400.0;
    const CLIENT_H: f32 = 250.0;

    struct State {
        name: Vec<u16>,
        end: Instant,
        scale: f32,
    }

    thread_local! {
        static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            WM_ERASEBKGND => 1,
            WM_TIMER => {
                InvalidateRect(hwnd, null(), 0);
                0
            }
            WM_PAINT => {
                paint(hwnd);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }

    unsafe fn paint(hwnd: HWND) {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rc);
        let (w, h) = (rc.right, rc.bottom);
        let mem = CreateCompatibleDC(hdc);
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let old = SelectObject(mem, bmp);
        STATE.with(|s| {
            if let Some(st) = s.borrow().as_ref() {
                draw(mem, w, h, st);
            }
        });
        BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old);
        DeleteObject(bmp);
        DeleteDC(mem);
        EndPaint(hwnd, &ps);
    }

    unsafe fn text(hdc: HDC, s: &str, rect: &mut RECT, px: i32, weight: i32, color: u32, flags: u32) {
        let face = wide("Segoe UI");
        let font = CreateFontW(-px, 0, 0, 0, weight, 0, 0, 0, DEFAULT_CHARSET as u32, 0, 0, CLEARTYPE_QUALITY as u32, 0, face.as_ptr());
        let old = SelectObject(hdc, font);
        SetTextColor(hdc, color);
        let t = wide(s);
        DrawTextW(hdc, t.as_ptr(), -1, rect, flags);
        SelectObject(hdc, old);
        DeleteObject(font);
    }

    unsafe fn draw(hdc: HDC, w: i32, h: i32, st: &State) {
        let left = st.end.saturating_duration_since(Instant::now());
        let s = st.scale;
        let px = |v: f32| (v * s).round() as i32;

        let bg = CreateSolidBrush(BG);
        FillRect(hdc, &RECT { left: 0, top: 0, right: w, bottom: h }, bg);
        DeleteObject(bg);

        SetBkMode(hdc, TRANSPARENT as i32);
        let flags = DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX;
        let line = |top, bottom| RECT {
            left: px(24.0),
            top: px(top),
            right: w - px(24.0),
            bottom: px(bottom),
        };
        let name = String::from_utf16_lossy(&st.name[..st.name.len().saturating_sub(1)]);
        text(hdc, &name, &mut line(59.0, 86.0), px(19.0), 600, INK, flags | DT_END_ELLIPSIS);
        text(hdc, "Time remaining", &mut line(94.0, 115.0), px(14.0), 400, MUTED, flags);
        let clock = format_clock(left.as_secs_f64().ceil() as u64);
        text(hdc, &clock, &mut line(117.0, 181.0), px(52.0), 400, INK, flags);
    }

    pub fn run(title: &str, duration: Duration) -> bool {
        let end = Instant::now() + duration;
        let class = wide("OrbshackerFakeGame");
        let title_w = wide(title);
        unsafe {
            SetProcessDPIAware();
            let scale = (GetDpiForSystem().max(96)) as f32 / 96.0;
            STATE.with(|s| *s.borrow_mut() = Some(State { name: title_w.clone(), end, scale }));

            let hinst = GetModuleHandleW(null());
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(wndproc);
            wc.hInstance = hinst;
            wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
            wc.lpszClassName = class.as_ptr();
            RegisterClassW(&wc);

            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
            let mut frame = RECT { left: 0, top: 0, right: (CLIENT_W * scale) as i32, bottom: (CLIENT_H * scale) as i32 };
            AdjustWindowRect(&mut frame, style, 0);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                title_w.as_ptr(),
                style,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                frame.right - frame.left,
                frame.bottom - frame.top,
                null_mut(),
                null_mut(),
                hinst,
                null(),
            );
            if hwnd.is_null() {
                STATE.with(|s| *s.borrow_mut() = None);
                return false;
            }
            let dark: i32 = 1;
            DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, &dark as *const i32 as *const _, 4);
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            UpdateWindow(hwnd);
            SetTimer(hwnd, 1, 1000, None);

            let mut msg: MSG = std::mem::zeroed();
            let mut quit = false;
            while !quit && Instant::now() < end {
                while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                    if msg.message == WM_QUIT {
                        quit = true;
                        break;
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                if quit {
                    break;
                }
                let left = end.saturating_duration_since(Instant::now());
                MsgWaitForMultipleObjects(0, null(), 0, left.as_millis().min(250) as u32, QS_ALLINPUT);
            }
            if !quit {
                DestroyWindow(hwnd);
                PeekMessageW(&mut msg, null_mut(), WM_QUIT, WM_QUIT, PM_REMOVE);
            }
            STATE.with(|s| *s.borrow_mut() = None);
        }
        true
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowLongW, GetWindowRect, IsIconic, IsWindowVisible, PostMessageW, GWL_EXSTYLE, GWL_STYLE,
        WM_CLOSE, WS_CAPTION, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn find(title: &str, secs: u64) -> HWND {
        let t = wide(title);
        let end = Instant::now() + Duration::from_secs(secs);
        let mut hwnd: HWND = std::ptr::null_mut();
        while Instant::now() < end && hwnd.is_null() {
            hwnd = unsafe { FindWindowW(std::ptr::null(), t.as_ptr()) };
            std::thread::sleep(Duration::from_millis(50));
        }
        hwnd
    }

    #[test]
    fn it_is_a_normal_visible_application_window() {
        let title = "orbshacker-test-window-7f3a";
        let handle = std::thread::spawn(move || run_game_window(title, Duration::from_secs(3)));
        let hwnd = find(title, 3);
        assert!(!hwnd.is_null(), "a top-level window with the fake's title exists");
        unsafe {
            assert_ne!(IsWindowVisible(hwnd), 0, "visible");
            assert_eq!(IsIconic(hwnd), 0, "not minimised");
            assert_ne!(GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION, 0, "has a title bar");
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            assert_eq!(ex & WS_EX_TOOLWINDOW, 0, "not a tool window, so it has a taskbar button");
            assert_eq!(ex & WS_EX_LAYERED, 0, "not transparent");
            assert_eq!(ex & WS_EX_NOACTIVATE, 0, "a normal window you can click");
            let mut r: RECT = std::mem::zeroed();
            GetWindowRect(hwnd, &mut r);
            assert!(r.right - r.left >= 380 && r.bottom - r.top >= 230, "a real window size, got {}x{}", r.right - r.left, r.bottom - r.top);
        }
        assert!(handle.join().unwrap(), "the window ran to the end of its time");
    }

    #[test]
    fn closing_the_window_ends_the_process_early() {
        let title = "orbshacker-test-window-close-4be8";
        let started = Instant::now();
        let handle = std::thread::spawn(move || run_game_window(title, Duration::from_secs(60)));
        let hwnd = find(title, 3);
        assert!(!hwnd.is_null());
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
        assert!(handle.join().unwrap());
        assert!(started.elapsed() < Duration::from_secs(10), "returned long before the 60 s deadline");
    }

    #[test]
    fn the_window_goes_away_when_the_time_is_up() {
        let title = "orbshacker-test-window-short-91c2";
        assert!(run_game_window(title, Duration::from_millis(400)));
        let t = wide(title);
        assert!(unsafe { FindWindowW(std::ptr::null(), t.as_ptr()) }.is_null());
    }

    #[test]
    fn a_zero_length_timer_does_not_panic_or_hang() {
        assert!(run_game_window("orbshacker-test-window-zero-2d70", Duration::ZERO));
    }
}
