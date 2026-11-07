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

use std::cell::Cell;

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
    /// The shell has shown the Layers inspector: it draws native panels, so PhotoCraft's own
    /// Layers panel would show the same thing twice. (A shell describes only the panels it opened.)
    layers_shown: Cell<bool>,
    /// PhotoCraft's own Layers panel has been hidden since: it showing again is the user asking.
    layers_hidden: Cell<bool>,
}

impl Native {
    /// One panel, described from the app as it is now; `None` for a panel PhotoCraft doesn't
    /// offer (or Layers without a document).
    pub(crate) fn describe(&self, app: &PhotocraftApp, id: &PanelId) -> Option<Panel> {
        match id.as_str() {
            LAYERS => {
                self.layers_shown.set(true);
                layers::describe(app)
            }
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

    /// Hide PhotoCraft's own panels the shell draws natively, once the shell has shown that it
    /// does. Asking for one since (Window › Layers, the rail's Layers icon) gives the id of the
    /// native panel to open instead: for [`craft_mobile::Platform::open_panel`].
    pub(crate) fn hide_replaced_panels(&self, app: &mut PhotocraftApp) -> Option<&'static str> {
        if !self.layers_shown.get() {
            return None;
        }
        // Shown before the native inspector took over (the workspace's panel) isn't asked for.
        let asked = app.ui.panels.layers && self.layers_hidden.replace(true);
        app.ui.panels.layers = false;
        asked.then_some(LAYERS)
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

    /// PhotoCraft's own Layers panel stays until the shell shows the native one (the Compose
    /// shell doesn't draw native panels yet), and then for good.
    #[test]
    fn the_egui_layers_panel_goes_once_the_shell_shows_the_native_one() {
        let (mut app, native) = (app(), Native::default());
        app.ui.panels.layers = true;
        native.describe(&app, &BRUSH.into());
        assert_eq!(native.hide_replaced_panels(&mut app), None);
        assert!(app.ui.panels.layers, "the brush popover alone replaces nothing");
        native.describe(&app, &LAYERS.into());
        assert_eq!(native.hide_replaced_panels(&mut app), None, "the inspector is open already");
        assert!(!app.ui.panels.layers);
        assert_eq!(native.hide_replaced_panels(&mut app), None);
    }

    /// Window › Layers or the rail's Layers icon, once the native inspector replaces the panel:
    /// the shell is asked to open the inspector, once, and the egui panel stays hidden.
    #[test]
    fn asking_for_the_egui_layers_panel_opens_the_native_one() {
        let (mut app, native) = (app(), Native::default());
        native.describe(&app, &LAYERS.into());
        native.hide_replaced_panels(&mut app);
        app.ui.panels.layers = true;
        assert_eq!(native.hide_replaced_panels(&mut app), Some(LAYERS));
        assert!(!app.ui.panels.layers);
        assert_eq!(native.hide_replaced_panels(&mut app), None, "one request per ask");
    }

    #[test]
    fn fractions_clamp_and_refuse_nan() {
        assert_eq!(fraction(1.000_001), Ok(1.0));
        assert_eq!(fraction(-0.0001), Ok(0.0));
        assert!(fraction(f64::NAN).is_err());
    }
}
