use std::{cell::Cell, ops::Deref};

use rwh_06::RawWindowHandle;
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Shell::{
            Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_MODIFY, NOTIFYICONDATAW,
        },
        WindowsAndMessaging::{
            CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos,
            LoadIconW, PostMessageW, PostQuitMessage, RegisterClassExW, RegisterClassW,
            RegisterWindowMessageW, SetForegroundWindow, SetMenuInfo, CREATESTRUCTW, CS_HREDRAW,
            CS_VREDRAW, CW_USEDEFAULT, GWL_USERDATA, HICON, IDI_APPLICATION, MENUINFO,
            MIM_APPLYTOSUBMENUS, MIM_STYLE, MNS_NOTIFYBYPOS, WM_CREATE, WM_DESTROY,
            WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN,
            WM_MBUTTONUP, WM_MENUCOMMAND, WM_MOUSEMOVE, WM_NCCREATE, WM_RBUTTONDBLCLK,
            WM_RBUTTONDOWN, WM_RBUTTONUP, WM_USER, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP,
            WNDCLASSEXW, WNDCLASSW, WS_OVERLAPPEDWINDOW,
        },
    },
};

use crate::{
    dpi::PhysicalPosition,
    error::OsError as RootOsError,
    event::Event,
    platform_impl::platform::{event_loop::ProcResult, WindowId, DEVICE_ID},
    tray::TrayBuilder,
    window::{Icon, WindowId as RootWindowId},
};

use super::{
    event_loop::{runner::EventLoopRunnerShared, DESTROY_MSG_ID},
    util, EventLoopWindowTarget,
};

#[derive(Clone)]
pub struct Tray(HWND);

impl Tray {
    pub fn new<T: 'static>(
        tray_builder: TrayBuilder,
        event_loop: &EventLoopWindowTarget<T>,
    ) -> Result<Tray, RootOsError> {
        let tray = init_window::<T>(tray_builder.parent_window, tray_builder.tooltip, event_loop)?;
        if let Some(icon) = tray_builder.icon {
            tray.set_icon(icon)?;
        }
        Ok(tray)
    }

    pub fn id(&self) -> RootWindowId {
        RootWindowId(WindowId(**self))
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

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            // The window must be destroyed from the same thread that created it, so we send a
            // custom message to be handled by our callback to do the actual work.
            PostMessageW(self.0, DESTROY_MSG_ID.get(), 0, 0);
        }
    }
}

impl Deref for Tray {
    type Target = HWND;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct InitData<'a, T: 'static> {
    pub event_loop: &'a EventLoopWindowTarget<T>,
    // outputs
    pub window: Option<HWND>,
}

impl<'a, T: 'static> InitData<'a, T> {
    unsafe fn on_nccreate(&mut self, window: HWND) -> Option<isize> {
        let runner = self.event_loop.runner_shared.clone();

        let result = runner.catch_unwind(|| {
            let window_data = WindowData {
                event_loop_runner: self.event_loop.runner_shared.clone(),
                userdata_removed: Cell::new(false),
                recurse_depth: Cell::new(0),
            };
            window_data
        });
        result.map(|userdata| {
            self.window = Some(window);
            let userdata = Box::into_raw(Box::new(userdata));
            userdata as _
        })
    }
}

pub(crate) struct WindowData<T: 'static> {
    pub event_loop_runner: EventLoopRunnerShared<T>,
    pub userdata_removed: Cell<bool>,
    pub recurse_depth: Cell<u32>,
}
impl<T> WindowData<T> {
    fn send_event(&self, event: Event<T>) {
        self.event_loop_runner.send_event(event);
    }
}

pub fn init_window<T: 'static>(
    parent_window: Option<RawWindowHandle>,
    tooltip: Option<String>,
    event_loop: &EventLoopWindowTarget<T>,
) -> Result<Tray, RootOsError> {
    let hmodule = unsafe { GetModuleHandleW(std::ptr::null()) };
    if hmodule == 0 {
        return Err(os_error!(std::io::Error::last_os_error()));
    }

    let class_name = util::encode_wide("my_window");

    let wnd = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc::<T>),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: util::get_instance_handle(),
        hIcon: 0,
        hCursor: 0, // must be null in order for cursor state to work properly
        hbrBackground: 0,
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_name.as_ptr(),
        hIconSm: 0,
    };

    unsafe { RegisterClassExW(&wnd) };

    let parent_hwnd = match parent_window {
        Some(RawWindowHandle::Win32(handle)) => Some(handle.hwnd.get()),
        Some(_) => unreachable!("Invalid raw window handle {parent_window:?} on Windows"),
        _ => None,
    };

    let mut initdata = InitData {
        event_loop,
        window: None,
    };

    println!("hwnd: {:?}", initdata.window);

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            util::encode_wide(tooltip.unwrap_or("rust_systray_window".to_string())).as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            0,
            CW_USEDEFAULT,
            0,
            parent_hwnd.unwrap_or(0) as HWND,
            0,
            util::get_instance_handle(),
            &mut initdata as *mut _ as *mut _,
        )
    };

    // If the window creation in `InitData` panicked, then should resume panicking here
    if let Err(panic_error) = event_loop.runner_shared.take_panic_error() {
        std::panic::resume_unwind(panic_error)
    }

    if hwnd == 0 {
        return Err(os_error!(std::io::Error::last_os_error()));
    }

    // If the handle is non-null, then window creation must have succeeded, which means
    // that we *must* have populated the `InitData.window` field.
    // let win = initdata.window.unwrap();

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

pub(crate) extern "system" fn window_proc<T: 'static>(
    window: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    let userdata = unsafe { super::get_window_long(window, GWL_USERDATA) };

    let userdata_ptr = match (userdata, msg) {
        (0, WM_NCCREATE) => {
            let createstruct = unsafe { &mut *(l_param as *mut CREATESTRUCTW) };
            let initdata = unsafe { &mut *(createstruct.lpCreateParams as *mut InitData<'_, T>) };

            let result = match unsafe { initdata.on_nccreate(window) } {
                Some(userdata) => unsafe {
                    super::set_window_long(window, GWL_USERDATA, userdata as _);
                    DefWindowProcW(window, msg, w_param, l_param)
                },
                None => -1, // failed to create the window
            };
            return result;
        }
        // Getting here should quite frankly be impossible,
        // but we'll make window creation fail here just in case.
        (0, WM_CREATE) => return -1,
        (_, WM_CREATE) => unsafe {
            let createstruct = &mut *(l_param as *mut CREATESTRUCTW);
            let initdata = createstruct.lpCreateParams;
            let initdata = &mut *(initdata as *mut InitData<'_, T>);

            return DefWindowProcW(window, msg, w_param, l_param);
        },
        (0, _) => return unsafe { DefWindowProcW(window, msg, w_param, l_param) },
        _ => userdata as *mut WindowData<T>,
    };

    let (result, userdata_removed, recurse_depth) = {
        let userdata = unsafe { &*(userdata_ptr) };

        userdata.recurse_depth.set(userdata.recurse_depth.get() + 1);

        let result =
            unsafe { public_window_callback_inner(window, msg, w_param, l_param, userdata) };

        let userdata_removed = userdata.userdata_removed.get();
        let recurse_depth = userdata.recurse_depth.get() - 1;
        userdata.recurse_depth.set(recurse_depth);

        (result, userdata_removed, recurse_depth)
    };

    if userdata_removed && recurse_depth == 0 {
        drop(unsafe { Box::from_raw(userdata_ptr) });
    }

    result
}

unsafe fn public_window_callback_inner<T: 'static>(
    window: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
    userdata: &WindowData<T>,
) -> LRESULT {
    let mut result = ProcResult::DefWindowProc(w_param);

    match msg {
        1025 if (l_param as u32 == WM_LBUTTONUP
            || l_param as u32 == WM_RBUTTONUP
            || l_param as u32 == WM_MBUTTONUP
            || l_param as u32 == WM_XBUTTONUP
            || l_param as u32 == WM_LBUTTONDOWN
            || l_param as u32 == WM_RBUTTONDOWN
            || l_param as u32 == WM_MBUTTONDOWN
            || l_param as u32 == WM_XBUTTONDOWN) =>
        {
            let (button, state) = match l_param as u32 {
                x if x == WM_LBUTTONUP => (
                    crate::event::MouseButton::Left,
                    crate::event::ElementState::Released,
                ),
                x if x == WM_RBUTTONUP => (
                    crate::event::MouseButton::Right,
                    crate::event::ElementState::Released,
                ),
                x if x == WM_MBUTTONUP => (
                    crate::event::MouseButton::Middle,
                    crate::event::ElementState::Released,
                ),
                x if x == WM_XBUTTONUP => (
                    crate::event::MouseButton::Other(0),
                    crate::event::ElementState::Released,
                ),
                x if x == WM_LBUTTONDOWN => (
                    crate::event::MouseButton::Left,
                    crate::event::ElementState::Pressed,
                ),
                x if x == WM_RBUTTONDOWN => (
                    crate::event::MouseButton::Right,
                    crate::event::ElementState::Pressed,
                ),
                x if x == WM_MBUTTONDOWN => (
                    crate::event::MouseButton::Middle,
                    crate::event::ElementState::Pressed,
                ),
                x if x == WM_XBUTTONDOWN => (
                    crate::event::MouseButton::Other(0),
                    crate::event::ElementState::Pressed,
                ),
                _ => unreachable!("Invalid mouse button event"),
            };

            use crate::event::WindowEvent::{CursorMoved, MouseInput};
            let mut point = POINT { x: 0, y: 0 };
            if unsafe { GetCursorPos(&mut point) } == 0 {
                return 1;
            }
            let position = PhysicalPosition::new(point.x as f64, point.y as f64);

            userdata.send_event(Event::WindowEvent {
                window_id: RootWindowId(WindowId(window)),
                event: CursorMoved {
                    device_id: DEVICE_ID,
                    position,
                },
            });

            userdata.send_event(Event::WindowEvent {
                window_id: RootWindowId(WindowId(window)),
                event: MouseInput {
                    device_id: DEVICE_ID,
                    state,
                    button,
                },
            });

            result = ProcResult::Value(0);
        }

        _ => {
            if msg == DESTROY_MSG_ID.get() {
                unsafe { DestroyWindow(window) };
                result = ProcResult::Value(0);
            } else {
                result = ProcResult::DefWindowProc(w_param);
            }
        }
    };

    match result {
        ProcResult::DefWindowProc(wparam) => unsafe {
            DefWindowProcW(window, msg, wparam, l_param)
        },
        ProcResult::Value(val) => val,
    }
}
