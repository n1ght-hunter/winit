use std::ops::Deref;

use rwh_06::RawWindowHandle;
use windows_sys::Win32::{
    Foundation::HWND,
    Foundation::{LPARAM, LRESULT, POINT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        DefWindowProcW, GetCursorPos, PostQuitMessage, RegisterWindowMessageW, SetForegroundWindow,
        WM_CREATE, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONUP,
        WM_MENUCOMMAND, WM_RBUTTONDBLCLK, WM_RBUTTONUP, WM_XBUTTONDBLCLK, WM_XBUTTONUP,
    },
    UI::{
        Shell::{
            Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_MODIFY, NOTIFYICONDATAW,
        },
        WindowsAndMessaging::{
            CreatePopupMenu, CreateWindowExW, LoadIconW, RegisterClassW, SetMenuInfo,
            CW_USEDEFAULT, HICON, IDI_APPLICATION, MENUINFO, MIM_APPLYTOSUBMENUS, MIM_STYLE,
            MNS_NOTIFYBYPOS, WM_USER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
        },
    },
};

use crate::{error::OsError as RootOsError, window::Icon};

use super::util;

#[derive(Clone)]
pub struct Tray(HWND);

impl Tray {
    pub fn new<T: 'static>() -> Result<Tray, RootOsError> {
        init_window::<T>(None)
    }
    pub fn with_parent<T: 'static>(parent_hwnd: RawWindowHandle) -> Result<Tray, RootOsError> {
        let parent_hwnd = match parent_hwnd {
            RawWindowHandle::Win32(handle) => handle.hwnd,
            _ => unreachable!("Invalid raw window handle {parent_hwnd:?} on Windows"),
        };
        init_window::<T>(Some(parent_hwnd.get()))
    }

    pub fn set_icon(&self, icon: Icon) -> Result<(), RootOsError> {
        let icon = icon.inner.as_raw_handle();
        let mut icon_data = unsafe { std::mem::zeroed::<NOTIFYICONDATAW>() };
        icon_data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        icon_data.hWnd = **self;
        icon_data.uID = 1;
        icon_data.uFlags = NIF_ICON;
        icon_data.hIcon = icon;

        unsafe {
            if Shell_NotifyIconW(NIM_MODIFY, &icon_data) == 0 {
                return Err(os_error!(std::io::Error::last_os_error()));
            }
        }
        Ok(())
    }

    pub fn set_tooltip(&self, tooltip: &str) -> Result<(), RootOsError> {
        let wide_tooltip = util::encode_wide(tooltip);
        if wide_tooltip.len() > 128 {
            // return Err("The tooltip may not exceed 127 wide bytes");
            return Err(os_error!(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "The tooltip may not exceed 127 wide bytes"
            )));
        }

        let mut nid = unsafe { std::mem::zeroed::<NOTIFYICONDATAW>() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = **self;
        nid.uID = 1;
        nid.uFlags = NIF_TIP;

        #[cfg(target_arch = "x86")]
        {
            let mut tip_data = [0u16; 128];
            tip_data[..wide_tooltip.len()].copy_from_slice(&wide_tooltip);
            nid.szTip = tip_data;
        }

        #[cfg(not(target_arch = "x86"))]
        nid.szTip[..wide_tooltip.len()].copy_from_slice(&wide_tooltip);

        unsafe {
            if Shell_NotifyIconW(NIM_MODIFY, &nid) == 0 {
                return Err(os_error!(std::io::Error::last_os_error()));
            }
        }
        Ok(())
    }
}

unsafe impl Send for Tray {}
unsafe impl Sync for Tray {}

impl Deref for Tray {
    type Target = HWND;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn init_window<T: 'static>(parent_hwnd: Option<HWND>) -> Result<Tray, RootOsError> {
    let hmodule = unsafe { GetModuleHandleW(std::ptr::null()) };
    if hmodule == 0 {
        return Err(os_error!(std::io::Error::last_os_error()));
    }

    let class_name = util::encode_wide("my_window");

    let mut wnd = unsafe { std::mem::zeroed::<WNDCLASSW>() };
    wnd.lpfnWndProc = Some(window_proc);
    // wnd.lpfnWndProc = Some(super::event_loop::public_window_callback::<T>);
    wnd.lpszClassName = class_name.as_ptr();

    unsafe { RegisterClassW(&wnd) };
    println!("print");

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            util::encode_wide("rust_systray_window").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            0,
            CW_USEDEFAULT,
            0,
            parent_hwnd.unwrap_or(0) as HWND,
            0,
            0,
            std::ptr::null(),
        )
    };
    if hwnd == 0 {
        return Err(os_error!(std::io::Error::last_os_error()));
    }

    let icon: HICON = unsafe {
        let mut handle = LoadIconW(
            GetModuleHandleW(std::ptr::null()),
            util::encode_wide("tray-default").as_ptr(),
        );
        if handle == 0 {
            handle = LoadIconW(0, IDI_APPLICATION);
        }
        if handle == 0 {
            return Err(os_error!(std::io::Error::last_os_error()));
        }
        handle as HICON
    };

    let mut nid = unsafe { std::mem::zeroed::<NOTIFYICONDATAW>() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_MESSAGE | NIF_ICON;
    nid.hIcon = icon;
    nid.uCallbackMessage = WM_USER + 1;

    if unsafe { Shell_NotifyIconW(NIM_ADD, &nid) } == 0 {
        return Err(os_error!(std::io::Error::last_os_error()));
    }

    // Setup menu
    let mut info = unsafe { std::mem::zeroed::<MENUINFO>() };
    info.cbSize = std::mem::size_of::<MENUINFO>() as u32;
    info.fMask = MIM_APPLYTOSUBMENUS | MIM_STYLE;
    info.dwStyle = MNS_NOTIFYBYPOS;
    let hmenu = unsafe { CreatePopupMenu() };
    if hmenu == 0 {
        return Err(os_error!(std::io::Error::last_os_error()));
    }
    if unsafe { SetMenuInfo(hmenu, &info) } == 0 {
        return Err(os_error!(std::io::Error::last_os_error()));
    }

    Ok(Tray(hwnd))
}

#[derive(Debug)]
pub enum Click {
    Single(crate::event::MouseButton),
    Double(crate::event::MouseButton),
}

#[derive(Debug)]
pub struct TrayEvent {
    mouse: Click,
    position: (i32, i32),
}

pub(crate) extern "system" fn window_proc(
    h_wnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    static mut U_TASKBAR_RESTART: u32 = 0;

    if msg == WM_MENUCOMMAND {
        // WININFO_STASH.with(|stash| {
        //     let stash = stash.borrow();
        //     let stash = stash.as_ref();
        //     if let Some(stash) = stash {
        //         let menu_id = GetMenuItemID(stash.info.hmenu, w_param as i32) as i32;
        //         if menu_id != -1 {
        //             stash.tx.send(WindowsTrayEvent(menu_id as u32)).ok();
        //         }
        //     }
        // });
    }

    // if msg == WM_USER + 1 && l_param as u32 != WM_MOUSEMOVE {
    //     println!("l_param: {}", l_param);
    //     println!("w_param: {}", w_param);
    // }

    {
        let l_param = l_param as u32;

        if msg == WM_USER + 1
            && (l_param == WM_LBUTTONUP
                || l_param == WM_LBUTTONDBLCLK
                || l_param == WM_RBUTTONUP
                || l_param == WM_RBUTTONDBLCLK
                || l_param == WM_MBUTTONUP
                || l_param == WM_MBUTTONDBLCLK
                || l_param == WM_XBUTTONUP
                || l_param == WM_XBUTTONDBLCLK)
        {
            let mut point = POINT { x: 0, y: 0 };
            if unsafe { GetCursorPos(&mut point) } == 0 {
                return 1;
            }

            let click = match l_param as u32 {
                WM_LBUTTONUP => Click::Single(crate::event::MouseButton::Left),
                WM_RBUTTONUP => Click::Single(crate::event::MouseButton::Right),
                WM_MBUTTONUP => Click::Single(crate::event::MouseButton::Middle),
                WM_XBUTTONUP => Click::Single(crate::event::MouseButton::Forward),
                WM_LBUTTONDBLCLK => Click::Double(crate::event::MouseButton::Left),
                WM_RBUTTONDBLCLK => Click::Double(crate::event::MouseButton::Right),
                WM_MBUTTONDBLCLK => Click::Double(crate::event::MouseButton::Middle),
                WM_XBUTTONDBLCLK => Click::Double(crate::event::MouseButton::Forward),
                _ => panic!("shouldnt be anything other than left or right click"),
            };

            let event = TrayEvent {
                mouse: click,
                position: (point.x, point.y),
            };

            println!("{:?}", event);

            unsafe { SetForegroundWindow(h_wnd) };

            // WININFO_STASH.with(|stash| {
            //     let stash = stash.borrow();
            //     let stash = stash.as_ref();
            //     if let Some(stash) = stash {
            //         TrackPopupMenu(
            //             stash.info.hmenu,
            //             TPM_LEFTBUTTON | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
            //             point.x,
            //             point.y,
            //             0,
            //             h_wnd,
            //             ptr::null(),
            //         );
            //     }
            // });
        }
    }

    if msg == WM_CREATE {
        unsafe {
            U_TASKBAR_RESTART = RegisterWindowMessageW(util::encode_wide("TaskbarCreated").as_ptr())
        };
    }

    // If windows explorer restarts and we need to recreate the tray icon
    if msg == unsafe { U_TASKBAR_RESTART } {
        let icon: HICON = unsafe {
            let mut handle = LoadIconW(
                GetModuleHandleW(std::ptr::null()),
                util::encode_wide("tray-default").as_ptr(),
            );
            if handle == 0 {
                handle = LoadIconW(0, IDI_APPLICATION);
            }
            if handle == 0 {
                println!("Error setting icon from resource");
                PostQuitMessage(0);
            }
            handle as HICON
        };
        let mut nid = unsafe { std::mem::zeroed::<NOTIFYICONDATAW>() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = h_wnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON;
        nid.hIcon = icon;
        nid.uCallbackMessage = WM_USER + 1;
        if unsafe { Shell_NotifyIconW(NIM_ADD, &nid) } == 0 {
            println!("Error adding menu icon");
            unsafe { PostQuitMessage(0) };
        }
    }

    if msg == WM_DESTROY {
        unsafe { PostQuitMessage(0) };
    }

    unsafe { DefWindowProcW(h_wnd, msg, w_param, l_param) }
}
