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
mod documents;
mod layers;
pub(crate) mod menu;
mod rail;
pub(crate) mod status;

use craft_mobile::ui::{Edit, Feature, Panel, PanelId, PanelInfo, PanelRole, ShellFeatures};
use photocraft_ui_egui::{Drawn, HostChrome, PhotocraftApp};
use serde_json::Value;

pub(crate) use layers::Drags;

/// The Layers inspector's id.
const LAYERS: &str = "layers";
/// The brush popover's id.
const BRUSH: &str = "brush";
/// The Documents inspector's id.
const DOCUMENTS: &str = "documents";
/// The tool rail's id.
const TOOLS: &str = "tools";

/// PhotoCraft's own panels the shell draws natively, as (`panel_docking::PANELS` id, native panel
/// id). The brush popover replaces none of them: it is the brush's settings, not the Brushes
/// panel's presets.
const NATIVE_PANELS: [(&str, &str); 1] = [("layers", LAYERS)];

/// The native panel that stands for PhotoCraft's panel `id`, if any: for
/// [`craft_mobile::Platform::open_panel`] when Window › Layers asks for it.
pub(crate) fn native_panel(id: &str) -> Option<&'static str> {
    NATIVE_PANELS.iter().find(|(own, _)| *own == id).map(|(_, native)| *native)
}

/// The parts of PhotoCraft's chrome a shell drawing `features` takes over; `None` for a shell
/// that draws none of it. The rail stands for the toolbar and the options bar (its tools' options
/// open from it); the panels for the dock and the document tabs (the Documents inspector).
pub(crate) fn host_chrome(features: ShellFeatures) -> Option<HostChrome> {
    if features.is_empty() {
        return None;
    }
    let drawn = |feature| if features.contains(feature) { Drawn::ByHost } else { Drawn::ByPhotoCraft };
    let panels = drawn(Feature::Panels);
    Some(HostChrome {
        menus: drawn(Feature::Menus),
        toolbar: drawn(Feature::Rail),
        options_bar: drawn(Feature::Rail),
        status_bar: drawn(Feature::Status),
        tabs: panels,
        dock: panels,
        native_panels: if panels == Drawn::ByHost { NATIVE_PANELS.iter().map(|(own, _)| (*own).to_owned()).collect() } else { Vec::new() },
    })
}

/// The native panels' state between frames.
#[derive(Debug, Default)]
pub(crate) struct Native {
    /// The Layers inspector's drags in progress.
    drags: Drags,
    /// What the shell draws: the Documents inspector and the rail are offered only to a shell that
    /// draws the panels and the rail.
    features: ShellFeatures,
}

impl Native {
    /// The shell draws `features`.
    pub(crate) fn shell(&mut self, features: ShellFeatures) {
        self.features = features;
    }

    /// Whether the shell draws `feature`.
    pub(crate) fn draws(&self, feature: Feature) -> bool {
        self.features.contains(feature)
    }

    /// The panels PhotoCraft offers the shell.
    pub(crate) fn panels(&self) -> Vec<PanelInfo> {
        let mut panels = vec![
            PanelInfo { id: LAYERS.into(), title: "Layers".into(), icon: "square.3.layers.3d".into(), role: PanelRole::Inspector },
            PanelInfo { id: BRUSH.into(), title: "Brush".into(), icon: "paintbrush.pointed".into(), role: PanelRole::Tool },
        ];
        if self.features.contains(Feature::Panels) {
            panels.push(PanelInfo { id: DOCUMENTS.into(), title: "Documents".into(), icon: "doc.on.doc".into(), role: PanelRole::Inspector });
        }
        if self.features.contains(Feature::Rail) {
            panels.push(PanelInfo { id: TOOLS.into(), title: "Tools".into(), icon: "hammer".into(), role: PanelRole::Rail });
        }
        panels
    }

    /// One panel, described from the app as it is now; `None` for a panel PhotoCraft doesn't
    /// offer (or Layers without a document).
    pub(crate) fn describe(&self, app: &PhotocraftApp, id: &PanelId) -> Option<Panel> {
        match id.as_str() {
            LAYERS => layers::describe(app),
            BRUSH => Some(brush::describe(app)),
            DOCUMENTS => Some(documents::describe(app)),
            TOOLS => rail::describe(app),
            _ => None,
        }
    }

    /// Apply one edit from a native panel.
    pub(crate) fn apply(&mut self, app: &mut PhotocraftApp, ctx: &egui::Context, edit: &Edit) -> Result<(), String> {
        match edit.panel.as_str() {
            LAYERS => layers::apply(app, &mut self.drags, ctx, edit),
            BRUSH => brush::apply(app, ctx, edit),
            DOCUMENTS => documents::apply(app, ctx, edit),
            TOOLS => rail::apply(app, ctx, edit),
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

    fn every_feature() -> ShellFeatures {
        ShellFeatures::from(Feature::ALL)
    }

    #[test]
    fn every_offered_panel_is_described_and_well_formed() {
        let (app, mut native) = (app(), Native::default());
        native.shell(every_feature());
        let offered = native.panels();
        assert_eq!(offered.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), [LAYERS, BRUSH, DOCUMENTS, TOOLS]);
        for info in offered {
            let panel = native.describe(&app, &info.id).unwrap();
            assert_eq!(panel.id(), &info.id);
            panel.check().unwrap();
        }
        assert!(native.describe(&app, &"timeline".into()).is_none());
    }

    /// A shell that draws neither the panels nor the rail (the Compose shell, for now) isn't
    /// offered the Documents inspector or the rail: PhotoCraft's tabs and toolbar are on screen.
    #[test]
    fn documents_and_the_rail_are_for_a_shell_that_draws_them() {
        let native = Native::default();
        assert_eq!(native.panels().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), [LAYERS, BRUSH]);
    }

    #[test]
    fn a_shell_with_every_feature_takes_over_all_the_chrome() {
        let chrome = host_chrome(every_feature()).unwrap();
        assert_eq!(
            chrome,
            HostChrome {
                menus: Drawn::ByHost,
                toolbar: Drawn::ByHost,
                options_bar: Drawn::ByHost,
                status_bar: Drawn::ByHost,
                tabs: Drawn::ByHost,
                dock: Drawn::ByHost,
                native_panels: vec!["layers".into()],
            }
        );
        assert_eq!(host_chrome(ShellFeatures::none()), None, "no features: PhotoCraft draws everything");
        let menus_only = host_chrome(ShellFeatures::from([Feature::Menus])).unwrap();
        assert_eq!(menus_only, HostChrome { menus: Drawn::ByHost, ..HostChrome::default() });
        let rail = host_chrome(ShellFeatures::from([Feature::Rail])).unwrap();
        assert_eq!((rail.toolbar, rail.options_bar, rail.dock), (Drawn::ByHost, Drawn::ByHost, Drawn::ByPhotoCraft));
    }

    /// The panels the host draws natively are PhotoCraft panel ids it knows, each with a native
    /// panel PhotoCraft offers.
    #[test]
    fn native_panels_map_to_offered_panels() {
        let mut native = Native::default();
        native.shell(every_feature());
        let offered: Vec<String> = native.panels().into_iter().map(|p| p.id.as_str().to_owned()).collect();
        for id in host_chrome(every_feature()).unwrap().native_panels {
            let mapped = native_panel(&id).unwrap();
            assert!(offered.iter().any(|o| o == mapped), "{id} → {mapped}");
        }
        assert_eq!(native_panel("brushes"), None, "the brush popover isn't the Brushes panel");
        assert_eq!(native_panel("color"), None);
    }

    #[test]
    fn fractions_clamp_and_refuse_nan() {
        assert_eq!(fraction(1.000_001), Ok(1.0));
        assert_eq!(fraction(-0.0001), Ok(0.0));
        assert!(fraction(f64::NAN).is_err());
    }
}
