use crate::Icon;
use iced::{Color, widget::svg};
use iced_adaptive_design::{IconSource, IconView, Theme};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

impl Icon {
    /// A cached handle for Weave-owned artwork, never a system-theme substitute.
    pub fn handle(self) -> svg::Handle {
        static HANDLES: OnceLock<Mutex<HashMap<Icon, svg::Handle>>> = OnceLock::new();
        HANDLES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(self)
            .or_insert_with(|| svg::Handle::from_memory(self.source().as_bytes()))
            .clone()
    }
    /// Follows the containing widget's foreground, including selected/disabled states.
    pub fn view<'a>(self, size: f32) -> IconView<'a> {
        IconSource::from(self).view(size)
    }
    pub fn colored<'a>(self, size: f32, color: Color) -> IconView<'a> {
        self.view(size)
            .style(move |_: &Theme, _| svg::Style { color: Some(color) })
    }
}
impl From<Icon> for IconSource {
    fn from(icon: Icon) -> Self {
        Self::Svg(icon.handle())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supplied_artwork_is_used_instead_of_system_resolution() {
        let first = Icon::Play.handle();
        assert_eq!(first, Icon::Play.handle());
        assert_ne!(first, Icon::Pause.handle());
        assert!(matches!(IconSource::from(Icon::Star), IconSource::Svg(_)));
    }
}
