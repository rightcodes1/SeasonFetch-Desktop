#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::sync::Once;
#[cfg(windows)]
use windows_sys::Win32::Foundation::{HWND, RECT};
#[cfg(windows)]
use windows_sys::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
#[cfg(windows)]
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, RegisterClassW, SetWindowPos,
    ShowWindow, CS_HREDRAW, CS_VREDRAW, HWND_BOTTOM, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
    WNDCLASSW, WS_CHILD, WS_CLIPSIBLINGS, WS_DISABLED,
};

#[cfg(windows)]
static REGISTER_CLASS: Once = Once::new();

#[cfg(windows)]
fn wide_null(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// Dedicated click-through Win32 child HWND hosted inside Tauri's main window at `HWND_BOTTOM`,
/// strictly underneath the transparent WebView2 composition layer so React HUD controls remain
/// interactive on top of hardware-decoded D3D11 video frames.
#[cfg(windows)]
pub struct Win32VideoSurface {
    parent_hwnd: isize,
    child_hwnd: isize,
}

#[cfg(windows)]
impl Win32VideoSurface {
    pub fn create(parent_hwnd: isize) -> Result<Self, String> {
        if parent_hwnd == 0 {
            return Err("Invalid parent window handle (HWND=0)".to_string());
        }

        let class_name = wide_null("SeasonFetchNativeVideoSurface");
        unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());
            REGISTER_CLASS.call_once(|| {
                let wc = WNDCLASSW {
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(DefWindowProcW),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: hinstance,
                    hIcon: std::ptr::null_mut(),
                    hCursor: std::ptr::null_mut(),
                    hbrBackground: GetStockObject(BLACK_BRUSH) as HBRUSH,
                    lpszMenuName: std::ptr::null(),
                    lpszClassName: class_name.as_ptr(),
                };
                RegisterClassW(&wc);
            });

            let parent = parent_hwnd as HWND;
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 1280,
                bottom: 720,
            };
            let _ = GetClientRect(parent, &mut rect);
            let width = (rect.right - rect.left).max(1);
            let height = (rect.bottom - rect.top).max(1);

            // WS_DISABLED makes the surface 100% click-through so all mouse/keyboard input
            // stays routed to SeasonFetch's transparent WebView2 overlay.
            let child = CreateWindowExW(
                0,
                class_name.as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_CLIPSIBLINGS | WS_DISABLED,
                0,
                0,
                width,
                height,
                parent,
                std::ptr::null_mut(),
                hinstance,
                std::ptr::null(),
            );

            if child.is_null() {
                return Err("Win32 CreateWindowExW failed for embedded video surface".to_string());
            }

            SetWindowPos(
                child,
                HWND_BOTTOM,
                0,
                0,
                width,
                height,
                SWP_NOACTIVATE,
            );

            Ok(Self {
                parent_hwnd,
                child_hwnd: child as isize,
            })
        }
    }

    pub fn raw_hwnd(&self) -> isize {
        self.child_hwnd
    }

    pub fn show_and_fit(&self) {
        unsafe {
            let parent = self.parent_hwnd as HWND;
            let child = self.child_hwnd as HWND;
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 1280,
                bottom: 720,
            };
            let _ = GetClientRect(parent, &mut rect);
            let width = (rect.right - rect.left).max(1);
            let height = (rect.bottom - rect.top).max(1);
            SetWindowPos(
                child,
                HWND_BOTTOM,
                0,
                0,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    pub fn sync_bounds(&self) {
        unsafe {
            let parent = self.parent_hwnd as HWND;
            let child = self.child_hwnd as HWND;
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 1280,
                bottom: 720,
            };
            if GetClientRect(parent, &mut rect) != 0 {
                let width = (rect.right - rect.left).max(1);
                let height = (rect.bottom - rect.top).max(1);
                SetWindowPos(
                    child,
                    HWND_BOTTOM,
                    0,
                    0,
                    width,
                    height,
                    SWP_NOACTIVATE,
                );
            }
        }
    }

    pub fn hide(&self) {
        unsafe {
            ShowWindow(self.child_hwnd as HWND, SW_HIDE);
        }
    }
}

#[cfg(windows)]
impl Drop for Win32VideoSurface {
    fn drop(&mut self) {
        if self.child_hwnd != 0 {
            unsafe {
                DestroyWindow(self.child_hwnd as HWND);
            }
            self.child_hwnd = 0;
        }
    }
}
