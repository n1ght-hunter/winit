
use rwh_06::RawWindowHandle;

use crate::{error::OsError, platform_impl};


pub struct Tray(platform_impl::Tray);

impl Tray {
    pub fn new<T: 'static>() -> Result<Tray, OsError> {
        platform_impl::Tray::new::<T>().map(Tray)
    }
    pub fn with_parent<T: 'static>(parent_hwnd: RawWindowHandle) -> Result<Tray, OsError> {
        platform_impl::Tray::with_parent::<T>(parent_hwnd).map(Tray)
    }

    pub fn set_icon(&self, icon: crate::window::Icon) -> Result<(), OsError> {
        self.0.set_icon(icon)
    }

    pub fn set_tooltip(&self, tooltip: &str) -> Result<(), OsError> {
        self.0.set_tooltip(tooltip)
    }
}