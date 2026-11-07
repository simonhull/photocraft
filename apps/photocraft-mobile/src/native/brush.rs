//! The brush popover: size, opacity and flow, and Shape Dynamics' size control (Pen Pressure,
//! Pen Tilt…).
//!
//! It edits the session brush, which is the current tool's brush (the egui UI keeps a copy per
//! painting tool and saves the session brush into it on a tool switch), so what it sets is what
//! paints. The brush is a tool option, not part of the document: its edits make no history step.

use craft_mobile::ui::{Choice, ChoiceOption, Edit, EditValue, Node, Number, Panel, PanelRole, Phase, Scale, Section, Toggle, Unit};
use photocraft_engine::paint::MAX_BRUSH_SIZE;
use photocraft_engine::paint::brush::Control;
use photocraft_ui_egui::PhotocraftApp;
use serde_json::{Value, json};

use super::{fraction, run};

/// Every control, as (control, its name in a brush patch, label). The first six are what a size
/// can follow; the rest are Angle's, shown only when the brush already has one.
const CONTROLS: [(Control, &str, &str); 8] = [
    (Control::Off, "off", "Off"),
    (Control::Fade, "fade", "Fade"),
    (Control::PenPressure, "penPressure", "Pen Pressure"),
    (Control::PenTilt, "penTilt", "Pen Tilt"),
    (Control::StylusWheel, "stylusWheel", "Stylus Wheel"),
    (Control::Rotation, "rotation", "Rotation"),
    (Control::InitialDirection, "initialDirection", "Initial Direction"),
    (Control::Direction, "direction", "Direction"),
];

/// How many of [`CONTROLS`] a size offers.
const SIZE_CONTROLS: usize = 6;

/// The brush's smallest size the popover sets, in pixels.
const MIN_SIZE: f64 = 1.0;

pub(super) fn describe(app: &PhotocraftApp) -> Panel {
    let b = &app.session.tools.brush;
    let dynamics = &b.shape_dynamics;
    let mut tip = Vec::with_capacity(3);
    // A log scale: 1 px to 5000 px on a linear slider leaves the sizes people paint with in its
    // first few points.
    if let Ok(size) = Number::new("size", "Size", MIN_SIZE..=f64::from(MAX_BRUSH_SIZE), f64::from(b.size)).and_then(|n| n.scale(Scale::Log)) {
        tip.push(Node::Number(size.unit(Unit::Pixels).step(1.0)));
    }
    for (id, label, value) in [("opacity", "Opacity", b.opacity), ("flow", "Flow", b.flow)] {
        if let Ok(n) = Number::new(id, label, 0.0..=1.0, f64::from(value)) {
            tip.push(Node::Number(n.unit(Unit::Percent)));
        }
    }
    let mut shape = vec![Node::Toggle(Toggle::new("shape_dynamics", "Shape Dynamics", dynamics.enabled))];
    let current = CONTROLS.iter().find(|(c, ..)| *c == dynamics.size.control).map_or("off", |(_, id, _)| *id);
    let options: Vec<ChoiceOption> = CONTROLS
        .iter()
        .enumerate()
        .filter(|(i, (_, id, _))| *i < SIZE_CONTROLS || *id == current)
        .map(|(_, (_, id, label))| ChoiceOption::new(*id, *label))
        .collect();
    if let Ok(choice) = Choice::new("size_control", "Size Control", current, options) {
        shape.push(Node::Choice(choice.enabled(dynamics.enabled)));
    }
    Panel::new(
        super::BRUSH,
        "Brush",
        PanelRole::Tool,
        vec![Node::Section(Section::new("tip", "Tip", tip)), Node::Section(Section::new("dynamics", "Shape Dynamics", shape))],
    )
}

/// Apply an edit as a `tools.setBrush` patch. A cancelled drag carries the value from before it,
/// so it applies like any other: the brush has no history step to take back.
pub(super) fn apply(app: &mut PhotocraftApp, ctx: &egui::Context, edit: &Edit) -> Result<(), String> {
    if edit.phase == Phase::Begin {
        return Ok(());
    }
    run(app, ctx, "tools.setBrush", patch(edit)?, edit)
}

/// The brush patch for an edit.
fn patch(edit: &Edit) -> Result<Value, String> {
    Ok(match (&edit.value, edit.control.as_str()) {
        (EditValue::Number(v), "size") if v.is_finite() => json!({"size": v.clamp(MIN_SIZE, f64::from(MAX_BRUSH_SIZE))}),
        (EditValue::Number(v), "opacity") => json!({"opacity": fraction(*v)?}),
        (EditValue::Number(v), "flow") => json!({"flow": fraction(*v)?}),
        (EditValue::Toggle(on), "shape_dynamics") => json!({"shapeDynamics": {"enabled": on}}),
        (EditValue::Choice(c), "size_control") => {
            if !CONTROLS.iter().any(|(_, id, _)| id == c) {
                return Err(format!("brush: unknown control `{c}`"));
            }
            json!({"shapeDynamics": {"size": {"control": c}}})
        }
        (value, control) => return Err(format!("brush: `{control}` doesn't take {value:?}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_mobile::ui::{EditValue, Phase};
    use photocraft_engine::Session;
    use photocraft_engine::paint::brush::Control;
    use photocraft_ui_egui::Services;
    use serde_json::{Value, json};

    fn app() -> (PhotocraftApp, egui::Context) {
        (PhotocraftApp::new(Session::new(), Services::default()), egui::Context::default())
    }

    fn edit(control: &str, phase: Phase, gesture: u64, value: EditValue) -> Edit {
        Edit { panel: "brush".into(), control: control.into(), phase, gesture: gesture.into(), value }
    }

    fn commit(control: &str, value: EditValue) -> Edit {
        edit(control, Phase::Commit, 1, value)
    }

    /// The node with `id`, sections searched.
    fn node<'a>(nodes: &'a Value, id: &str) -> Option<&'a Value> {
        nodes.as_array()?.iter().find_map(|n| if n["id"] == id { Some(n) } else { node(&n["children"], id) })
    }

    fn described(app: &PhotocraftApp) -> Value {
        let panel = describe(app);
        panel.check().unwrap();
        serde_json::to_value(panel).unwrap()["nodes"].clone()
    }

    #[test]
    fn describes_size_opacity_flow_and_the_size_control() {
        let (app, _) = app();
        let nodes = described(&app);
        let b = &app.session.tools.brush;
        assert_eq!(node(&nodes, "size").unwrap()["value"], json!(f64::from(b.size)), "{nodes}");
        assert_eq!(node(&nodes, "size").unwrap()["unit"], "pixels");
        assert_eq!(node(&nodes, "size").unwrap()["scale"], "log", "1 px to 5000 px: small sizes get room");
        assert!(node(&nodes, "opacity").unwrap().get("scale").is_none(), "linear, the default, isn't sent");
        for id in ["opacity", "flow"] {
            assert_eq!(node(&nodes, id).unwrap()["unit"], "percent", "{id}");
        }
        assert_eq!(node(&nodes, "shape_dynamics").unwrap()["value"], json!(b.shape_dynamics.enabled));
        let control = node(&nodes, "size_control").unwrap();
        assert_eq!(control["enabled"], json!(b.shape_dynamics.enabled), "greyed out with Shape Dynamics off");
        let ids: Vec<&str> = control["options"].as_array().unwrap().iter().map(|o| o["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["off", "fade", "penPressure", "penTilt", "stylusWheel", "rotation"]);
    }

    /// The popover is titled Brush: its sections don't say it again.
    #[test]
    fn sections_dont_repeat_the_title() {
        let (app, _) = app();
        let titles: Vec<String> = described(&app).as_array().unwrap().iter().map(|n| n["title"].as_str().unwrap().to_owned()).collect();
        assert_eq!(titles, ["Tip", "Shape Dynamics"]);
    }

    /// An option's id is the control's own name in a brush patch.
    #[test]
    fn size_control_ids_are_the_brush_patch_names() {
        for (control, id, _) in CONTROLS {
            assert_eq!(serde_json::to_value(control).unwrap(), json!(id));
        }
    }

    /// A size control only an Angle can have (set elsewhere) still shows as it is.
    #[test]
    fn an_unusual_size_control_is_shown_as_it_is() {
        let (mut app, _) = app();
        app.session.tools.brush.shape_dynamics.size.control = Control::Direction;
        let nodes = described(&app);
        let control = node(&nodes, "size_control").unwrap();
        assert_eq!(control["value"], "direction", "{control}");
    }

    #[test]
    fn pen_tilt_reaches_the_brush() {
        let (mut app, ctx) = app();
        apply(&mut app, &ctx, &commit("shape_dynamics", EditValue::Toggle(true))).unwrap();
        apply(&mut app, &ctx, &commit("size_control", EditValue::Choice("penTilt".into()))).unwrap();
        let b = &app.session.tools.brush;
        assert!(b.shape_dynamics.enabled);
        assert_eq!(b.shape_dynamics.size.control, Control::PenTilt);
        assert_eq!(node(&described(&app), "size_control").unwrap()["value"], "penTilt");
    }

    #[test]
    fn size_opacity_and_flow_apply() {
        let (mut app, ctx) = app();
        apply(&mut app, &ctx, &commit("size", EditValue::Number(42.0))).unwrap();
        apply(&mut app, &ctx, &commit("opacity", EditValue::Number(0.5))).unwrap();
        apply(&mut app, &ctx, &commit("flow", EditValue::Number(0.25))).unwrap();
        let b = &app.session.tools.brush;
        assert!((b.size - 42.0).abs() < 1e-6 && (b.opacity - 0.5).abs() < 1e-6 && (b.flow - 0.25).abs() < 1e-6);
    }

    /// The brush is a tool option, not part of the document: no history step.
    #[test]
    fn a_size_drag_leaves_the_document_alone_and_cancel_puts_it_back() {
        let (mut app, ctx) = app();
        app.session.execute("file.new", json!({"width": 8, "height": 8})).unwrap();
        let (revision, size) = (app.session.active().unwrap().revision, app.session.tools.brush.size);
        apply(&mut app, &ctx, &edit("size", Phase::Begin, 5, EditValue::Number(f64::from(size)))).unwrap();
        apply(&mut app, &ctx, &edit("size", Phase::Change, 5, EditValue::Number(300.0))).unwrap();
        assert!((app.session.tools.brush.size - 300.0).abs() < 1e-6);
        apply(&mut app, &ctx, &edit("size", Phase::Cancel, 5, EditValue::Number(f64::from(size)))).unwrap();
        assert!((app.session.tools.brush.size - size).abs() < 1e-6);
        assert_eq!(app.session.active().unwrap().revision, revision);
    }

    #[test]
    fn rejected_edits_leave_the_brush_alone() {
        let (mut app, ctx) = app();
        let before = app.session.tools.brush.clone();
        assert!(apply(&mut app, &ctx, &commit("size_control", EditValue::Choice("telepathy".into()))).is_err());
        assert!(apply(&mut app, &ctx, &commit("size", EditValue::Toggle(true))).is_err());
        assert!(apply(&mut app, &ctx, &commit("size", EditValue::Number(f64::NAN))).is_err());
        assert!(apply(&mut app, &ctx, &commit("hardness", EditValue::Number(0.5))).is_err());
        assert_eq!(app.session.tools.brush, before);
    }
}
