//! The Layers inspector: the active layer's blend mode and opacity, then the layers, top first.
//!
//! As in the desktop Layers panel, the Background has no blend mode or opacity (both greyed out;
//! setting one would turn it into Layer 0), and Pass Through is offered for groups only.

use craft_mobile::ui::{Choice, ChoiceOption, Edit, EditValue, ItemList, Node, Number, Panel, PanelRole, Phase, Position, Row, Unit};
use photocraft_color::BlendMode;
use photocraft_doc::{Layer, LayerContent, LayerId};
use photocraft_ui_egui::PhotocraftApp;
use serde_json::json;

use super::{coalesce_key, fraction, run};

/// The blend mode menu.
const BLEND: &str = "blend";
/// The Opacity slider.
const OPACITY: &str = "opacity";
/// The layer rows.
const ROWS: &str = "rows";

pub(super) fn describe(app: &PhotocraftApp) -> Option<Panel> {
    let st = app.session.active()?;
    let doc = &st.doc;
    let active = st.active_layer.and_then(|id| doc.layer(id));
    // No layer, or the Background: nothing to set (Photoshop greys them, at Normal and 100 %).
    let editable = active.is_some_and(|l| !doc.is_background(l.id));
    let (blend, opacity, group) = active.map_or((BlendMode::Normal, 1.0, false), |l| (l.blend, f64::from(l.opacity), l.is_group()));
    let mut nodes = Vec::with_capacity(3);
    if let Ok(choice) = Choice::new(BLEND, "Blend Mode", blend.label(), blend_options(group, blend)) {
        nodes.push(Node::Choice(choice.enabled(editable)));
    }
    if let Ok(number) = Number::new(OPACITY, "Opacity", 0.0..=1.0, opacity) {
        nodes.push(Node::Number(number.unit(Unit::Percent).enabled(editable)));
    }
    let mut rows = Vec::new();
    let selected = |id| st.active_layer == Some(id) || st.selected_layers.contains(&id);
    push_rows(&doc.layers, 0, &selected, &mut rows);
    nodes.push(Node::ItemList(ItemList::new(ROWS, rows).reorderable(true)));
    Some(Panel::new(super::LAYERS, "Layers", PanelRole::Inspector, nodes))
}

/// The modes the menu offers, as the desktop panel's: Pass Through for a group, Photoshop's
/// modes, and a Paint.NET mode the layer already has. An option's id is the mode's label, which
/// `layer.setProps` takes as its `blend`.
fn blend_options(group: bool, current: BlendMode) -> Vec<ChoiceOption> {
    let own = Some(current).filter(|m| !m.has_psd_equivalent());
    std::iter::once(BlendMode::PassThrough)
        .filter(|_| group)
        .chain(BlendMode::LAYER_MODES)
        .chain(own)
        .map(|m| ChoiceOption::new(m.label(), m.label()))
        .collect()
}

/// Layers top first, as the Layers panel lists them (documents store them bottom first), with
/// each group's children under it.
fn push_rows(layers: &[Layer], depth: u32, selected: &dyn Fn(LayerId) -> bool, out: &mut Vec<Row>) {
    for layer in layers.iter().rev() {
        out.push(
            Row::new(layer.id.0, layer.name.as_str())
                .subtitle(layer.content.kind_name())
                .depth(depth)
                .visible(Some(layer.visible))
                .selected(selected(layer.id)),
        );
        if let LayerContent::Group(group) = &layer.content {
            push_rows(&group.children, depth + 1, selected, out);
        }
    }
}

pub(super) fn apply(app: &mut PhotocraftApp, ctx: &egui::Context, edit: &Edit) -> Result<(), String> {
    match (edit.phase, &edit.value, edit.control.as_str()) {
        // Begin carries the value from before the drag: nothing to do yet.
        (Phase::Begin, ..) => Ok(()),
        (Phase::Cancel, value, _) => cancel(app, ctx, edit, value),
        (_, EditValue::Number(v), OPACITY) => {
            let layer = active(app)?;
            run(app, ctx, "layer.setProps", json!({"layer": layer, "opacity": fraction(*v)?}), edit)
        }
        (_, EditValue::Choice(mode), BLEND) => {
            let layer = active(app)?;
            run(app, ctx, "layer.setProps", json!({"layer": layer, "blend": mode}), edit)
        }
        (_, EditValue::Select { row }, ROWS) => run(app, ctx, "layer.select", json!({"layer": row}), edit),
        (_, EditValue::SetVisible { row, visible }, ROWS) => run(app, ctx, "layer.setProps", json!({"layer": row, "visible": visible}), edit),
        (_, EditValue::Rename { row, name }, ROWS) => run(app, ctx, "layer.renameLayer", json!({"layer": row, "name": name}), edit),
        (_, EditValue::Move { row, target, position }, ROWS) => {
            let position = match position {
                Position::Above => "above",
                Position::Below => "below",
            };
            run(app, ctx, "layer.moveTo", json!({"layer": row, "target": target, "position": position}), edit)
        }
        (_, value, control) => Err(format!("layers: `{control}` doesn't take {value:?}")),
    }
}

/// The active layer's id.
fn active(app: &PhotocraftApp) -> Result<u64, String> {
    app.session.active().and_then(|st| st.active_layer).map(|id| id.0).ok_or_else(|| "no active layer".into())
}

/// An abandoned drag: when the latest history step is the gesture's own, take it back, leaving
/// neither a step to undo nor one to redo. Otherwise (nothing was applied, or another edit came
/// in between) put the opacity from before the drag back if it changed.
fn cancel(app: &mut PhotocraftApp, ctx: &egui::Context, edit: &Edit, value: &EditValue) -> Result<(), String> {
    let key = coalesce_key(edit);
    let ours = app.session.active().is_some_and(|st| st.coalesce.as_deref() == Some(key.as_str()));
    if ours && app.session.rollback() {
        return Ok(());
    }
    match (value, edit.control.as_str()) {
        (EditValue::Number(v), OPACITY) => {
            let before = fraction(*v)?;
            let now = app.session.active().and_then(|st| st.active_layer.and_then(|id| st.doc.layer(id))).map(|l| f64::from(l.opacity));
            match now {
                Some(now) if (now - before).abs() > f64::from(f32::EPSILON) => {
                    let layer = active(app)?;
                    run(app, ctx, "layer.setProps", json!({"layer": layer, "opacity": before}), edit)
                }
                _ => Ok(()),
            }
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_mobile::ui::{Edit, EditValue, Phase};
    use photocraft_engine::Session;
    use photocraft_ui_egui::Services;
    use serde_json::{Value, json};

    fn app_with_two_layers() -> (PhotocraftApp, egui::Context, u64) {
        let mut session = Session::new();
        session.execute("file.new", json!({"width": 8, "height": 8})).unwrap();
        session.execute("layer.new.layer", json!({})).unwrap();
        let top = session.active().unwrap().active_layer.unwrap().0;
        (PhotocraftApp::new(session, Services::default()), egui::Context::default(), top)
    }

    fn edit(control: &str, phase: Phase, gesture: u64, value: EditValue) -> Edit {
        Edit { panel: "layers".into(), control: control.into(), phase, gesture, value }
    }

    fn layer(app: &PhotocraftApp, id: u64) -> &Layer {
        app.session.active().unwrap().doc.layer(LayerId(id)).unwrap()
    }

    /// The bottom layer's id (ids are unique across documents, so not 1).
    fn background(app: &PhotocraftApp) -> u64 {
        app.session.active().unwrap().doc.layers.first().unwrap().id.0
    }

    fn json(app: &PhotocraftApp) -> Value {
        let panel = describe(app).unwrap();
        panel.check().unwrap();
        serde_json::to_value(panel).unwrap()
    }

    fn option_ids(node: &Value) -> Vec<String> {
        node["options"].as_array().unwrap().iter().map(|o| o["id"].as_str().unwrap().to_owned()).collect()
    }

    #[test]
    fn describes_rows_top_first_with_the_active_layers_props() {
        let (app, _, top) = app_with_two_layers();
        let json = json(&app);
        assert_eq!(json["nodes"][0]["id"], "blend");
        assert_eq!(json["nodes"][0]["value"], "Normal");
        assert_eq!(json["nodes"][0]["enabled"], true);
        assert_eq!(json["nodes"][1]["id"], "opacity");
        assert_eq!(json["nodes"][1]["unit"], "percent");
        let rows = &json["nodes"][2]["rows"];
        assert_eq!(rows.as_array().unwrap().len(), 2, "{json}");
        assert_eq!(rows[0]["id"], json!(top), "{json}");
        assert_eq!(rows[0]["selected"], true);
        assert_eq!(rows[1]["title"], "Background");
        assert_eq!(rows[1]["selected"], false);
    }

    #[test]
    fn describing_is_deterministic() {
        let (app, _, _) = app_with_two_layers();
        assert_eq!(describe(&app), describe(&app));
    }

    #[test]
    fn no_document_no_layers_panel() {
        let app = PhotocraftApp::new(Session::new(), Services::default());
        assert!(describe(&app).is_none());
    }

    #[test]
    fn the_background_has_no_blend_mode_or_opacity() {
        let (mut app, ctx, _) = app_with_two_layers();
        let background = background(&app);
        assert!(app.session.active().unwrap().doc.is_background(LayerId(background)));
        apply(&mut app, &ctx, &edit("rows", Phase::Commit, 1, EditValue::Select { row: background })).unwrap();
        let json = json(&app);
        assert_eq!(json["nodes"][0]["enabled"], false, "{json}");
        assert_eq!(json["nodes"][1]["enabled"], false, "{json}");
    }

    #[test]
    fn pass_through_is_offered_for_groups_only() {
        let (mut app, ctx, _) = app_with_two_layers();
        assert!(!option_ids(&json(&app)["nodes"][0]).contains(&"Pass Through".to_owned()));
        app.session.execute("layer.new.group", json!({})).unwrap();
        let json = json(&app);
        assert_eq!(json["nodes"][0]["value"], "Pass Through", "a new group passes through");
        assert!(option_ids(&json["nodes"][0]).contains(&"Pass Through".to_owned()));
        apply(&mut app, &ctx, &edit("blend", Phase::Commit, 1, EditValue::Choice("Normal".into()))).unwrap();
    }

    /// The menu's ids are what `layer.setProps` takes, each naming its own mode.
    #[test]
    fn every_blend_option_is_a_mode_the_engine_takes() {
        for (group, current) in [(true, BlendMode::PassThrough), (false, BlendMode::Glow)] {
            let options = serde_json::to_value(blend_options(group, current)).unwrap();
            for option in options.as_array().unwrap() {
                let id = option["id"].as_str().unwrap();
                let mode = photocraft_engine::commands::blend_from_str(id).unwrap();
                assert_eq!(mode.label(), id);
            }
            assert!(option_ids(&json!({"options": options})).contains(&current.label().to_owned()));
        }
    }

    #[test]
    fn an_opacity_drag_is_one_undo_step() {
        let (mut app, ctx, top) = app_with_two_layers();
        for (phase, v) in [(Phase::Begin, 1.0), (Phase::Change, 0.7), (Phase::Change, 0.4), (Phase::Commit, 0.4)] {
            apply(&mut app, &ctx, &edit("opacity", phase, 9, EditValue::Number(v))).unwrap();
        }
        assert!((layer(&app, top).opacity - 0.4).abs() < 1e-6);
        app.session.execute("edit.undo", json!({})).unwrap();
        assert!((layer(&app, top).opacity - 1.0).abs() < 1e-6, "one undo reverts the whole drag");
        assert!(app.session.active().unwrap().doc.layer(LayerId(top)).is_some(), "and only the drag");
    }

    #[test]
    fn two_drags_are_two_steps() {
        let (mut app, ctx, top) = app_with_two_layers();
        apply(&mut app, &ctx, &edit("opacity", Phase::Commit, 1, EditValue::Number(0.5))).unwrap();
        apply(&mut app, &ctx, &edit("opacity", Phase::Commit, 2, EditValue::Number(0.25))).unwrap();
        app.session.execute("edit.undo", json!({})).unwrap();
        assert!((layer(&app, top).opacity - 0.5).abs() < 1e-6);
    }

    #[test]
    fn cancel_puts_the_value_back_and_leaves_no_step() {
        let (mut app, ctx, top) = app_with_two_layers();
        let revision = app.session.active().unwrap().revision;
        let undo_before = app.session.active().unwrap().history.can_undo();
        apply(&mut app, &ctx, &edit("opacity", Phase::Begin, 3, EditValue::Number(1.0))).unwrap();
        apply(&mut app, &ctx, &edit("opacity", Phase::Change, 3, EditValue::Number(0.2))).unwrap();
        apply(&mut app, &ctx, &edit("opacity", Phase::Cancel, 3, EditValue::Number(1.0))).unwrap();
        assert!((layer(&app, top).opacity - 1.0).abs() < 1e-6);
        let st = app.session.active().unwrap();
        assert!(st.revision > revision, "the canvas hears of it");
        assert!(!st.history.can_redo(), "nothing to redo");
        assert_eq!(st.history.can_undo(), undo_before);
        // The step before the drag is still the one Undo takes back: the new layer.
        app.session.execute("edit.undo", json!({})).unwrap();
        assert!(app.session.active().unwrap().doc.layer(LayerId(top)).is_none());
    }

    #[test]
    fn a_cancel_without_a_change_does_nothing() {
        let (mut app, ctx, _) = app_with_two_layers();
        let revision = app.session.active().unwrap().revision;
        apply(&mut app, &ctx, &edit("opacity", Phase::Begin, 4, EditValue::Number(1.0))).unwrap();
        apply(&mut app, &ctx, &edit("opacity", Phase::Cancel, 4, EditValue::Number(1.0))).unwrap();
        assert_eq!(app.session.active().unwrap().revision, revision);
    }

    #[test]
    fn blend_visibility_rename_and_select_apply() {
        let (mut app, ctx, top) = app_with_two_layers();
        apply(&mut app, &ctx, &edit("blend", Phase::Commit, 1, EditValue::Choice("Multiply".into()))).unwrap();
        apply(&mut app, &ctx, &edit("rows", Phase::Commit, 2, EditValue::SetVisible { row: top, visible: false })).unwrap();
        apply(&mut app, &ctx, &edit("rows", Phase::Commit, 3, EditValue::Rename { row: top, name: "Sky".into() })).unwrap();
        let l = layer(&app, top);
        assert_eq!((l.blend, l.visible, l.name.as_str()), (BlendMode::Multiply, false, "Sky"));
        let bottom = background(&app);
        apply(&mut app, &ctx, &edit("rows", Phase::Commit, 4, EditValue::Select { row: bottom })).unwrap();
        assert_eq!(app.session.active().unwrap().active_layer, Some(LayerId(bottom)));
    }

    #[test]
    fn a_row_moves_below_another() {
        let (mut app, ctx, top) = app_with_two_layers();
        app.session.execute("layer.new.layer", json!({})).unwrap();
        let third = app.session.active().unwrap().active_layer.unwrap().0;
        apply(&mut app, &ctx, &edit("rows", Phase::Commit, 1, EditValue::Move { row: third, target: top, position: Position::Below })).unwrap();
        let order: Vec<u64> = app.session.active().unwrap().doc.layers.iter().map(|l| l.id.0).collect();
        assert_eq!(order, [background(&app), third, top], "bottom first");
    }

    #[test]
    fn a_rejected_edit_changes_nothing() {
        let (mut app, ctx, _) = app_with_two_layers();
        let before = app.session.active().unwrap().revision;
        assert!(apply(&mut app, &ctx, &edit("blend", Phase::Commit, 1, EditValue::Choice("nonsense".into()))).is_err());
        assert!(apply(&mut app, &ctx, &edit("opacity", Phase::Commit, 2, EditValue::Toggle(true))).is_err());
        assert!(apply(&mut app, &ctx, &edit("opacity", Phase::Commit, 3, EditValue::Number(f64::NAN))).is_err());
        assert_eq!(app.session.active().unwrap().revision, before);
    }
}
