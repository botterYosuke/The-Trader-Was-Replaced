//! Catch-all editor toggles — Monaco `readOnly`, `contextmenu`, `links`,
//! `dragAndDrop`, `mouseStyle`, `accessibilitySupport`, `tabFocusMode`,
//! `automaticLayout`.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Clone, Debug, Serialize, Deserialize, Reflect)]
#[reflect(Component, Default, Debug)]
pub struct Misc {
    pub read_only: bool,
    pub contextmenu: bool,
    pub links: bool,
    pub drag_and_drop: bool,
    pub mouse_style: MouseStyle,
    pub accessibility_support: AccessibilitySupport,
    pub tab_focus_mode: bool,
    /// Mirrors Monaco `automaticLayout`. The equivalent in bevscode is the
    /// `AutoResizeViewport` Component; this field is surface-only.
    pub automatic_layout: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum MouseStyle {
    #[default]
    Text,
    Default,
    Copy,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[reflect(Debug, PartialEq)]
pub enum AccessibilitySupport {
    #[default]
    Auto,
    Off,
    On,
}

impl Default for Misc {
    fn default() -> Self {
        Self {
            read_only: false,
            contextmenu: true,
            links: true,
            drag_and_drop: true,
            mouse_style: MouseStyle::Text,
            accessibility_support: AccessibilitySupport::Auto,
            tab_focus_mode: false,
            automatic_layout: false,
        }
    }
}
