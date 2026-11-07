//! PhotoCraft's native panels (craft-mobile [`ui`](craft_mobile::ui)): described from the session,
//! edits mapped to engine commands, run as the egui UI runs them ([`menus::invoke`]).
//!
//! One drag is one undo step: every edit of a gesture carries the same `coalesce` key
//! ([`coalesce_key`]), which the engine folds into one history step (and one journal entry).
//! A `begin` applies nothing; the Layers inspector records there which layers the drag edits
//! ([`Drags`]), so the rest of it can't move to another layer.
//!
//! [`menus::invoke`]: photocraft_ui_egui::menus::invoke

mod brush;
mod layers;

use craft_mobile::ui::{Edit, Panel, PanelId, PanelInfo, PanelRole};
use photocraft_ui_egui::PhotocraftApp;
use serde_json::Value;

pub(crate) use layers::Drags;

/// The Layers inspector's id.
const LAYERS: &str = "layers";
/// The brush popover's id.
const BRUSH: &str = "brush";

/// The panels PhotoCraft offers the native shell.
pub(crate) fn panels() -> Vec<PanelInfo> {
    vec![
        PanelInfo { id: LAYERS.into(), title: "Layers".into(), icon: "square.3.layers.3d".into(), role: PanelRole::Inspector },
        PanelInfo { id: BRUSH.into(), title: "Brush".into(), icon: "paintbrush.pointed".into(), role: PanelRole::Tool },
    ]
}

/// The native panels' state between frames.
#[derive(Debug, Default)]
pub(crate) struct Native {
    /// The Layers inspector's drags in progress.
    drags: Drags,
}

impl Native {
    /// One panel, described from the app as it is now; `None` for a panel PhotoCraft doesn't
    /// offer (or Layers without a document).
    pub(crate) fn describe(&self, app: &PhotocraftApp, id: &PanelId) -> Option<Panel> {
        match id.as_str() {
            LAYERS => layers::describe(app),
            BRUSH => Some(brush::describe(app)),
            _ => None,
        }
    }

    /// Apply one edit from a native panel.
    pub(crate) fn apply(&mut self, app: &mut PhotocraftApp, ctx: &egui::Context, edit: &Edit) -> Result<(), String> {
        match edit.panel.as_str() {
            LAYERS => layers::apply(app, &mut self.drags, ctx, edit),
            BRUSH => brush::apply(app, ctx, edit),
            other => Err(format!("no native panel `{other}`")),
        }
    }
}

/// The key every edit of one gesture shares, so the engine folds them into one step.
fn coalesce_key(edit: &Edit) -> String {
    format!("native-{}-{}:{}", edit.panel, edit.control, edit.gesture)
}

/// Run an engine command as the UI does, in the edit's gesture (`params` must be an object).
fn run(app: &mut PhotocraftApp, ctx: &egui::Context, id: &str, mut params: Value, edit: &Edit) -> Result<(), String> {
    if let Value::Object(map) = &mut params {
        map.insert("coalesce".into(), Value::String(coalesce_key(edit)));
    }
    photocraft_ui_egui::menus::invoke(app, ctx, id, params).map(drop)
}

/// A slider's value as a 0..=1 fraction: the shell's float may land a hair outside the range,
/// which the engine would refuse.
fn fraction(v: f64) -> Result<f64, String> {
    if v.is_finite() { Ok(v.clamp(0.0, 1.0)) } else { Err(format!("{v} is not a number")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_engine::Session;
    use photocraft_ui_egui::Services;

    fn app() -> PhotocraftApp {
        let mut session = Session::new();
        session.execute("file.new", serde_json::json!({"width": 8, "height": 8})).unwrap();
        PhotocraftApp::new(session, Services::default())
    }

    #[test]
    fn every_offered_panel_is_described_and_well_formed() {
        let (app, native) = (app(), Native::default());
        for info in panels() {
            let panel = native.describe(&app, &info.id).unwrap();
            assert_eq!(panel.id(), &info.id);
            panel.check().unwrap();
        }
        assert!(native.describe(&app, &"timeline".into()).is_none());
    }

    #[test]
    fn fractions_clamp_and_refuse_nan() {
        assert_eq!(fraction(1.000_001), Ok(1.0));
        assert_eq!(fraction(-0.0001), Ok(0.0));
        assert!(fraction(f64::NAN).is_err());
    }
}
