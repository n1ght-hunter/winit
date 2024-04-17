use rwh_06::RawWindowHandle;

use crate::{error::OsError, event_loop::EventLoopWindowTarget, platform_impl, window::WindowId};

pub struct TrayBuilder {
    pub(crate) icon: Option<crate::window::Icon>,
    pub(crate) tooltip: Option<String>,
    pub(crate) parent_window: Option<RawWindowHandle>,
}

impl TrayBuilder {
    pub fn new() -> TrayBuilder {
        TrayBuilder {
            icon: None,
            tooltip: None,
            parent_window: None,
        }
    }

    pub fn with_icon(mut self, icon: crate::window::Icon) -> TrayBuilder {
        self.icon = Some(icon);
        self
    }

    pub fn with_tooltip(mut self, tooltip: &str) -> TrayBuilder {
        self.tooltip = Some(tooltip.to_string());
        self
    }

    pub fn parent_window(mut self, parent_window: RawWindowHandle) -> TrayBuilder {
        self.parent_window = Some(parent_window);
        self
    }

    pub fn build<T: 'static>(
        self,
        window_target: &EventLoopWindowTarget<T>,
    ) -> Result<Tray, OsError> {
        let tray = platform_impl::Tray::new::<T>(self, &window_target.p).map(Tray)?;

        Ok(tray)
    }
}

pub struct Tray(platform_impl::Tray);

impl Tray {

    pub fn id(&self) -> WindowId {
        self.0.id()
    }

    pub fn set_icon(&self, icon: crate::window::Icon) -> Result<(), OsError> {
        self.0.set_icon(icon)
    }

    pub fn set_tooltip(&self, tooltip: &str) -> Result<(), OsError> {
        self.0.set_tooltip(tooltip)
    }
}
