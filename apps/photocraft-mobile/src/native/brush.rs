//! The brush popover. Filled in by the next change.

use craft_mobile::ui::{Edit, Panel, PanelRole};
use photocraft_ui_egui::PhotocraftApp;

pub(super) fn describe(_app: &PhotocraftApp) -> Panel {
    Panel::new(super::BRUSH, "Brush", PanelRole::Tool, Vec::new())
}

pub(super) fn apply(_app: &mut PhotocraftApp, _ctx: &egui::Context, _edit: &Edit) -> Result<(), String> {
    Err("brush: not yet".into())
}
