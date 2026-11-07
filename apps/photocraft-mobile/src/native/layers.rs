//! The Layers inspector: the blend mode and opacity of the selected layers (showing the active
//! layer's), then the layers, top first.
//!
//! Each row's context menu has Duplicate and Delete (a swipe too) for that layer alone.
//!
//! As in the desktop Layers panel ([`prop_targets`]): with several layers selected, blend mode and
//! Opacity change every one of them but the Background; the Background alone has neither (both
//! greyed out, and refused: setting one would turn it into Layer 0); Pass Through is offered
//! when every target is a group.

use std::collections::HashMap;

use craft_mobile::ui::{
    Choice, ChoiceOption, Edit, EditValue, GestureId, ItemList, Node, Number, Panel, PanelRole, Phase, Position, Row, RowAction, SelectMode, Unit,
};
use photocraft_color::BlendMode;
use photocraft_doc::{DocId, Layer, LayerContent, LayerId};
use photocraft_engine::DocState;
use photocraft_ui_egui::PhotocraftApp;
use photocraft_ui_egui::panels::{blend_options, layer_drop_payload, prop_target_params, prop_targets, targets_are_groups};
use serde_json::{Value, json};

use super::{coalesce_key, fraction, run};

/// The blend mode menu.
const BLEND: &str = "blend";
/// The Opacity slider.
const OPACITY: &str = "opacity";
/// The layer rows.
const ROWS: &str = "rows";
/// A row's Duplicate: a copy of that layer (`layer.duplicate`).
const DUPLICATE: &str = "duplicate";
/// A row's Delete: that layer (`layer.delete`).
const DELETE: &str = "delete";

/// One Opacity drag, fixed at its `begin`: the document and layers it edits, each with its
/// opacity from before the drag. Selecting another layer mid-drag doesn't move the drag there.
#[derive(Debug)]
struct Drag {
    doc: DocId,
    targets: Vec<(LayerId, f32)>,
}

/// The Opacity drags in progress, by gesture. A drag ends with its `commit` or `cancel`.
#[derive(Debug, Default)]
pub(crate) struct Drags(HashMap<GestureId, Drag>);

pub(super) fn describe(app: &PhotocraftApp) -> Option<Panel> {
    let st = app.session.active()?;
    let doc = &st.doc;
    let active = st.active_layer.and_then(|id| doc.layer(id));
    // No layer, or the Background: nothing to set (Photoshop greys them, at Normal and 100 %).
    let editable = active.is_some_and(|l| !doc.is_background(l.id));
    let (blend, opacity) = active.map_or((BlendMode::Normal, 1.0), |l| (l.blend, f64::from(l.opacity)));
    let targets = active.map(|l| prop_targets(doc, l.id, &st.selected_layers())).unwrap_or_default();
    let mut options: Vec<ChoiceOption> = Vec::new();
    let mut shown = false;
    for (mode, label) in blend_options(!targets.is_empty() && targets_are_groups(doc, &targets), blend) {
        shown |= mode == blend;
        options.push(ChoiceOption::new(blend_key(mode), label));
    }
    if !shown {
        // A group's Pass Through with a layer selected beside it: shown, as the desktop menu does.
        options.push(ChoiceOption::new(blend_key(blend), blend.label()));
    }
    let mut nodes = Vec::with_capacity(3);
    if let Ok(choice) = Choice::new(BLEND, "Blend Mode", &blend_key(blend), options) {
        nodes.push(Node::Choice(choice.enabled(editable)));
    }
    if let Ok(number) = Number::new(OPACITY, "Opacity", 0.0..=1.0, opacity) {
        nodes.push(Node::Number(number.unit(Unit::Percent).enabled(editable)));
    }
    let mut rows = Vec::new();
    push_rows(&doc.layers, 0, &|id| st.is_layer_selected(id), &mut rows);
    nodes.push(Node::ItemList(ItemList::new(ROWS, rows).reorderable(true)));
    Some(Panel::new(super::LAYERS, "Layers", PanelRole::Inspector, nodes))
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
                .selected(selected(layer.id))
                .actions(vec![
                    RowAction::new(DUPLICATE, "Duplicate").icon(Some("plus.square.on.square")),
                    RowAction::new(DELETE, "Delete").icon(Some("trash")).destructive(true),
                ]),
        );
        if let LayerContent::Group(group) = &layer.content {
            push_rows(&group.children, depth + 1, selected, out);
        }
    }
}

pub(super) fn apply(app: &mut PhotocraftApp, drags: &mut Drags, ctx: &egui::Context, edit: &Edit) -> Result<(), String> {
    match (edit.phase, &edit.value, edit.control.as_str()) {
        (Phase::Begin, EditValue::Number(_), OPACITY) => begin(app, drags, edit),
        // Any other begin carries the value from before the gesture: nothing to do yet.
        (Phase::Begin, ..) => Ok(()),
        (Phase::Cancel, ..) => cancel(app, drags, ctx, edit),
        (phase, EditValue::Number(v), OPACITY) => {
            // A commit ends the drag even when it's refused.
            let ended = if phase == Phase::Commit { drags.0.remove(&edit.gesture) } else { None };
            let ids = drag_targets(app, ended.as_ref().or_else(|| drags.0.get(&edit.gesture)))?;
            set(app, ctx, &ids, "opacity", json!(fraction(*v)?), edit)
        }
        (_, EditValue::Choice(id), BLEND) => {
            let mode = blend_mode(id).ok_or_else(|| format!("layers: no blend mode `{id}`"))?;
            let (_, ids) = targets(active(app)?)?;
            set(app, ctx, &ids, "blend", json!(mode.label()), edit)
        }
        (_, EditValue::Select { row, mode }, ROWS) => {
            // The desktop Layers panel's plain, Command and Shift clicks.
            let mode = match mode {
                SelectMode::Replace => "replace",
                SelectMode::Toggle => "toggle",
                SelectMode::Range => "range",
            };
            run(app, ctx, "layer.select", json!({"layer": row, "mode": mode}), edit)
        }
        (_, EditValue::SetVisible { row, visible }, ROWS) => run(app, ctx, "layer.setProps", json!({"layer": row, "visible": visible}), edit),
        (_, EditValue::Rename { row, name }, ROWS) => run(app, ctx, "layer.renameLayer", json!({"layer": row, "name": name}), edit),
        (_, EditValue::Move { row, target, position }, ROWS) => {
            let position = match position {
                Position::Above => "above",
                Position::Below => "below",
            };
            // As the desktop's drag: a row of a multi-selection moves the whole selection.
            let selected = active(app)?.selected_layers();
            run(app, ctx, "layer.moveTo", layer_drop_payload(row.get(), LayerId(target.get()), position, &selected), edit)
        }
        // The row's layer alone, whatever is selected, as the row's menu says.
        (_, EditValue::Action { row, action }, ROWS) if action.as_str() == DUPLICATE => run(app, ctx, "layer.duplicate", json!({"layer": row}), edit),
        (_, EditValue::Action { row, action }, ROWS) if action.as_str() == DELETE => run(app, ctx, "layer.delete", json!({"layer": row}), edit),
        (_, value, control) => Err(format!("layers: `{control}` doesn't take {value:?}")),
    }
}

/// A blend mode's menu id: the engine's name for it in snake case (`normal`, `color_burn`,
/// `pass_through`), which `layer.setProps` also takes; stable, unlike the label.
fn blend_key(mode: BlendMode) -> String {
    let mut key = String::new();
    for c in format!("{mode:?}").chars() {
        if c.is_ascii_uppercase() && !key.is_empty() {
            key.push('_');
        }
        key.push(c.to_ascii_lowercase());
    }
    key
}

/// The blend mode a menu id ([`blend_key`]) names.
fn blend_mode(id: &str) -> Option<BlendMode> {
    std::iter::once(BlendMode::PassThrough).chain(BlendMode::layer_modes()).find(|m| blend_key(*m) == id)
}

/// The active document.
fn active(app: &PhotocraftApp) -> Result<&DocState, String> {
    app.session.active().ok_or_else(|| "no document".into())
}

/// The document and the layers blend mode and Opacity change now; refused while they're greyed
/// out (no active layer, or the Background).
fn targets(st: &DocState) -> Result<(DocId, Vec<LayerId>), String> {
    let layer = st.active_layer.ok_or("no active layer")?;
    if st.doc.is_background(layer) {
        return Err("the Background has no blend mode or opacity".into());
    }
    Ok((st.doc.id, prop_targets(&st.doc, layer, &st.selected_layers())))
}

/// A drag starts: fix its layers and their opacities.
fn begin(app: &PhotocraftApp, drags: &mut Drags, edit: &Edit) -> Result<(), String> {
    let st = active(app)?;
    let (doc, ids) = targets(st)?;
    let targets = ids.into_iter().filter_map(|id| st.doc.layer(id).map(|l| (id, l.opacity))).collect();
    drags.0.insert(edit.gesture, Drag { doc, targets });
    Ok(())
}

/// The layers a drag edits: those fixed at its begin, in the document it began in (another one
/// active since is refused, so the panel shows the app's value again); a value typed in (a commit
/// alone) edits the layers selected now.
fn drag_targets(app: &PhotocraftApp, drag: Option<&Drag>) -> Result<Vec<LayerId>, String> {
    let st = active(app)?;
    match drag {
        Some(drag) if drag.doc != st.doc.id => Err("another document is active since the drag began".into()),
        Some(drag) => Ok(drag.targets.iter().map(|(id, _)| *id).collect()),
        None => targets(st).map(|(_, ids)| ids),
    }
}

/// Set one property of `ids` in one `layer.setProps`.
fn set(app: &mut PhotocraftApp, ctx: &egui::Context, ids: &[LayerId], key: &str, value: Value, edit: &Edit) -> Result<(), String> {
    let mut params = prop_target_params(ids);
    if let Value::Object(map) = &mut params {
        map.insert(key.into(), value);
    }
    run(app, ctx, "layer.setProps", params, edit)
}

/// An abandoned drag. When the latest history step is the drag's own, take it back, leaving
/// neither a step to undo nor one to redo (refused when that can't happen now, e.g. while a job
/// runs, rather than leave a step that changes nothing). Then put back each layer's own opacity
/// that still differs, in one step: another step came in mid-drag, so the drag's changes before
/// it are in a step of their own (all of them, when the other step came last).
fn cancel(app: &mut PhotocraftApp, drags: &mut Drags, ctx: &egui::Context, edit: &Edit) -> Result<(), String> {
    // Nothing begun here (refused at its begin): nothing to put back.
    let Some(drag) = drags.0.remove(&edit.gesture) else { return Ok(()) };
    let st = active(app)?;
    if st.doc.id != drag.doc {
        return Err("another document is active since the drag began".into());
    }
    let key = coalesce_key(edit);
    if st.coalesce.as_deref() == Some(key.as_str()) && !app.session.rollback() {
        return Err("the drag can't be taken back while a job runs".into());
    }
    let st = active(app)?;
    let changed: Vec<(LayerId, f32)> =
        drag.targets.into_iter().filter(|(id, before)| st.doc.layer(*id).is_some_and(|l| (l.opacity - before).abs() > f32::EPSILON)).collect();
    for (id, before) in changed {
        run(app, ctx, "layer.setProps", json!({"layer": id.0, "opacity": before}), edit)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_engine::Session;
    use photocraft_ui_egui::Services;

    /// The inspector over a document with the Background and one layer above it, active.
    struct Inspector {
        app: PhotocraftApp,
        ctx: egui::Context,
        drags: Drags,
        top: u64,
    }

    impl Inspector {
        fn new() -> Self {
            let mut session = Session::new();
            session.execute("file.new", json!({"width": 8, "height": 8})).unwrap();
            session.execute("layer.new.layer", json!({})).unwrap();
            let top = session.active().unwrap().active_layer.unwrap().0;
            Self { app: PhotocraftApp::new(session, Services::default()), ctx: egui::Context::default(), drags: Drags::default(), top }
        }

        fn apply(&mut self, control: &str, phase: Phase, gesture: u64, value: EditValue) -> Result<(), String> {
            let edit = Edit { panel: "layers".into(), control: control.into(), phase, gesture: gesture.into(), value };
            apply(&mut self.app, &mut self.drags, &self.ctx, &edit)
        }

        /// A whole Opacity drag through `values`, ending in a commit of the last.
        fn drag(&mut self, gesture: u64, from: f64, values: &[f64]) {
            self.apply("opacity", Phase::Begin, gesture, EditValue::Number(from)).unwrap();
            for v in values {
                self.apply("opacity", Phase::Change, gesture, EditValue::Number(*v)).unwrap();
            }
            self.apply("opacity", Phase::Commit, gesture, EditValue::Number(*values.last().unwrap())).unwrap();
        }

        fn run(&mut self, id: &str, params: Value) {
            self.app.session.execute(id, params).unwrap();
        }

        fn select(&mut self, row: u64, mode: &str) {
            self.run("layer.select", json!({"layer": row, "mode": mode}));
        }

        fn state(&self) -> &DocState {
            self.app.session.active().unwrap()
        }

        fn layer(&self, id: u64) -> &Layer {
            self.state().doc.layer(LayerId(id)).unwrap()
        }

        fn opacity(&self, id: u64) -> f32 {
            self.layer(id).opacity
        }

        /// The bottom layer's id (ids are unique across documents, so not 1).
        fn background(&self) -> u64 {
            self.state().doc.layers.first().unwrap().id.0
        }

        /// A new layer above, active; its id.
        fn new_layer(&mut self) -> u64 {
            self.run("layer.new.layer", json!({}));
            self.state().active_layer.unwrap().0
        }

        fn undo(&mut self) {
            self.run("edit.undo", json!({}));
        }

        fn json(&self) -> Value {
            let panel = describe(&self.app).unwrap();
            panel.check().unwrap();
            serde_json::to_value(panel).unwrap()
        }
    }

    fn option_ids(node: &Value) -> Vec<String> {
        node["options"].as_array().unwrap().iter().map(|o| o["id"].as_str().unwrap().to_owned()).collect()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn describes_rows_top_first_with_the_active_layers_props() {
        let p = Inspector::new();
        let json = p.json();
        assert_eq!(json["nodes"][0]["id"], "blend");
        assert_eq!(json["nodes"][0]["value"], "normal");
        assert_eq!(json["nodes"][0]["enabled"], true);
        assert_eq!(json["nodes"][1]["id"], "opacity");
        assert_eq!(json["nodes"][1]["unit"], "percent");
        let rows = &json["nodes"][2]["rows"];
        assert_eq!(rows.as_array().unwrap().len(), 2, "{json}");
        assert_eq!(rows[0]["id"], json!(p.top), "{json}");
        assert_eq!(rows[0]["selected"], true);
        assert_eq!(rows[1]["title"], "Background");
        assert_eq!(rows[1]["selected"], false);
    }

    #[test]
    fn describing_is_deterministic() {
        let p = Inspector::new();
        assert_eq!(describe(&p.app), describe(&p.app));
    }

    #[test]
    fn no_document_no_layers_panel() {
        let app = PhotocraftApp::new(Session::new(), Services::default());
        assert!(describe(&app).is_none());
    }

    #[test]
    fn the_background_has_no_blend_mode_or_opacity_and_refuses_them() {
        let mut p = Inspector::new();
        let background = p.background();
        p.apply("rows", Phase::Commit, 1, EditValue::Select { row: background.into(), mode: SelectMode::Replace }).unwrap();
        let json = p.json();
        assert_eq!(json["nodes"][0]["enabled"], false, "{json}");
        assert_eq!(json["nodes"][1]["enabled"], false, "{json}");
        let revision = p.state().revision;
        assert!(p.apply("opacity", Phase::Begin, 2, EditValue::Number(1.0)).is_err());
        assert!(p.apply("opacity", Phase::Commit, 2, EditValue::Number(0.5)).is_err());
        assert!(p.apply("blend", Phase::Commit, 3, EditValue::Choice("multiply".into())).is_err());
        assert_eq!(p.state().revision, revision);
        assert!(p.state().doc.is_background(LayerId(background)), "still the Background");
    }

    #[test]
    fn pass_through_is_offered_when_every_target_is_a_group() {
        let mut p = Inspector::new();
        assert!(!option_ids(&p.json()["nodes"][0]).contains(&"pass_through".to_owned()));
        p.run("layer.new.group", json!({}));
        let json = p.json();
        assert_eq!(json["nodes"][0]["value"], "pass_through", "a new group passes through");
        assert!(option_ids(&json["nodes"][0]).contains(&"pass_through".to_owned()));
        // A layer selected beside the group: the menu still shows the group's mode.
        let top = p.top;
        p.select(top, "add");
        p.json();
        p.apply("blend", Phase::Commit, 1, EditValue::Choice("normal".into())).unwrap();
    }

    fn option_labels(node: &Value) -> Vec<String> {
        node["options"].as_array().unwrap().iter().map(|o| o["label"].as_str().unwrap().to_owned()).collect()
    }

    /// The menu's ids are the modes' stable keys, its labels what the user reads, and picking any
    /// option sets that mode.
    #[test]
    fn every_blend_option_round_trips_through_its_key() {
        let mut p = Inspector::new();
        p.run("layer.new.group", json!({}));
        let json = p.json();
        let (ids, labels) = (option_ids(&json["nodes"][0]), option_labels(&json["nodes"][0]));
        assert_eq!(json["nodes"][0]["value"], "pass_through");
        assert_eq!((ids.first().unwrap().as_str(), labels.first().unwrap().as_str()), ("pass_through", "Pass Through"));
        assert!(ids.contains(&"multiply".to_owned()) && ids.contains(&"color_burn".to_owned()), "{ids:?}");
        let group = p.state().active_layer.unwrap().0;
        for (n, (id, label)) in ids.iter().zip(&labels).enumerate() {
            let mode = blend_mode(id).unwrap();
            assert_eq!((blend_key(mode).as_str(), mode.label()), (id.as_str(), label.as_str()));
            p.apply("blend", Phase::Commit, 100 + n as u64, EditValue::Choice(id.clone())).unwrap();
            assert_eq!(p.layer(group).blend, mode, "{id}");
        }
        assert!(blend_mode("Multiply").is_none(), "a label isn't an id");
    }

    #[test]
    fn an_opacity_drag_is_one_undo_step() {
        let mut p = Inspector::new();
        let top = p.top;
        p.drag(9, 1.0, &[0.7, 0.4]);
        assert!(close(p.opacity(top), 0.4));
        p.undo();
        assert!(close(p.opacity(top), 1.0), "one undo reverts the whole drag");
        assert!(p.state().doc.layer(LayerId(top)).is_some(), "and only the drag");
        assert!(p.drags.0.is_empty(), "the drag is over");
    }

    #[test]
    fn two_drags_are_two_steps() {
        let mut p = Inspector::new();
        let top = p.top;
        p.drag(1, 1.0, &[0.5]);
        p.drag(2, 0.5, &[0.25]);
        p.undo();
        assert!(close(p.opacity(top), 0.5));
    }

    #[test]
    fn selecting_another_layer_mid_drag_doesnt_redirect_it() {
        let mut p = Inspector::new();
        let (top, below) = (p.top, p.background());
        let other = p.new_layer();
        p.apply("opacity", Phase::Begin, 4, EditValue::Number(1.0)).unwrap();
        p.apply("opacity", Phase::Change, 4, EditValue::Number(0.6)).unwrap();
        p.apply("rows", Phase::Commit, 5, EditValue::Select { row: top.into(), mode: SelectMode::Replace }).unwrap();
        p.apply("opacity", Phase::Commit, 4, EditValue::Number(0.3)).unwrap();
        assert!(close(p.opacity(other), 0.3));
        assert!(close(p.opacity(top), 1.0) && close(p.opacity(below), 1.0));
    }

    #[test]
    fn a_drag_refuses_another_document() {
        let mut p = Inspector::new();
        p.apply("opacity", Phase::Begin, 6, EditValue::Number(1.0)).unwrap();
        p.apply("opacity", Phase::Change, 6, EditValue::Number(0.5)).unwrap();
        p.run("file.new", json!({"width": 4, "height": 4}));
        p.new_layer();
        let revision = p.state().revision;
        assert!(p.apply("opacity", Phase::Change, 6, EditValue::Number(0.2)).is_err());
        assert!(p.apply("opacity", Phase::Commit, 6, EditValue::Number(0.2)).is_err());
        assert_eq!(p.state().revision, revision, "the new document is untouched");
    }

    /// A commit the app refuses still ends its drag.
    #[test]
    fn a_rejected_commit_ends_the_drag() {
        let mut p = Inspector::new();
        p.apply("opacity", Phase::Begin, 12, EditValue::Number(1.0)).unwrap();
        p.apply("opacity", Phase::Change, 12, EditValue::Number(0.5)).unwrap();
        p.run("file.new", json!({"width": 4, "height": 4}));
        assert!(p.apply("opacity", Phase::Commit, 12, EditValue::Number(0.2)).is_err());
        assert!(p.drags.0.is_empty(), "another document");
        p.new_layer();
        p.apply("opacity", Phase::Begin, 14, EditValue::Number(1.0)).unwrap();
        assert!(p.apply("opacity", Phase::Commit, 14, EditValue::Number(f64::NAN)).is_err());
        assert!(p.drags.0.is_empty(), "not a number");
    }

    #[test]
    fn cancel_puts_the_value_back_and_leaves_no_step() {
        let mut p = Inspector::new();
        let top = p.top;
        let (revision, could_undo) = (p.state().revision, p.state().history.can_undo());
        p.apply("opacity", Phase::Begin, 3, EditValue::Number(1.0)).unwrap();
        p.apply("opacity", Phase::Change, 3, EditValue::Number(0.2)).unwrap();
        p.apply("opacity", Phase::Cancel, 3, EditValue::Number(1.0)).unwrap();
        assert!(close(p.opacity(top), 1.0));
        assert!(p.state().revision > revision, "the canvas hears of it");
        assert!(!p.state().history.can_redo(), "nothing to redo");
        assert_eq!(p.state().history.can_undo(), could_undo);
        assert!(p.drags.0.is_empty());
        // The step before the drag is still the one Undo takes back: the new layer.
        p.undo();
        assert!(p.state().doc.layer(LayerId(top)).is_none());
    }

    #[test]
    fn cancel_after_another_step_restores_each_layers_own_opacity() {
        let mut p = Inspector::new();
        let top = p.top;
        let other = p.new_layer();
        p.run("layer.setProps", json!({"layer": top, "opacity": 0.3}));
        p.run("layer.setProps", json!({"layer": other, "opacity": 0.8}));
        p.select(top, "add");
        p.apply("opacity", Phase::Begin, 7, EditValue::Number(0.8)).unwrap();
        p.apply("opacity", Phase::Change, 7, EditValue::Number(0.5)).unwrap();
        assert!(close(p.opacity(top), 0.5) && close(p.opacity(other), 0.5));
        p.run("layer.renameLayer", json!({"layer": top, "name": "Sky"}));
        p.apply("opacity", Phase::Cancel, 7, EditValue::Number(0.8)).unwrap();
        assert!(close(p.opacity(top), 0.3) && close(p.opacity(other), 0.8));
        assert_eq!(p.layer(top).name, "Sky", "the step in between stays");
        p.undo();
        assert!(close(p.opacity(top), 0.5), "putting them back was one step");
    }

    /// The drag goes 100 → 60, a step from elsewhere (a stroke, a rename) becomes the latest, and the
    /// drag goes on to 40, which opens a second step under the drag's key. Cancel takes that one back
    /// and puts the 60 back to 100 too.
    #[test]
    fn cancel_after_a_step_from_elsewhere_mid_drag_undoes_the_whole_drag() {
        let mut p = Inspector::new();
        let top = p.top;
        p.apply("opacity", Phase::Begin, 11, EditValue::Number(1.0)).unwrap();
        p.apply("opacity", Phase::Change, 11, EditValue::Number(0.6)).unwrap();
        p.run("layer.renameLayer", json!({"layer": top, "name": "Sky"}));
        p.apply("opacity", Phase::Change, 11, EditValue::Number(0.4)).unwrap();
        p.apply("opacity", Phase::Cancel, 11, EditValue::Number(1.0)).unwrap();
        assert!(close(p.opacity(top), 1.0), "back from 60 as well as 40: {}", p.opacity(top));
        assert_eq!(p.layer(top).name, "Sky", "the step in between stays");
        assert!(p.drags.0.is_empty());
    }

    #[test]
    fn a_cancel_without_a_change_does_nothing() {
        let mut p = Inspector::new();
        let revision = p.state().revision;
        p.apply("opacity", Phase::Begin, 4, EditValue::Number(1.0)).unwrap();
        p.apply("opacity", Phase::Cancel, 4, EditValue::Number(1.0)).unwrap();
        assert_eq!(p.state().revision, revision);
    }

    #[test]
    fn a_drag_over_several_layers_changes_them_all_but_the_background_in_one_step() {
        let mut p = Inspector::new();
        let (top, background) = (p.top, p.background());
        let other = p.new_layer();
        // `add` makes the added layer active: the Background first, so it isn't.
        p.select(background, "add");
        p.select(top, "add");
        let json = p.json();
        let selected: Vec<bool> = json["nodes"][2]["rows"].as_array().unwrap().iter().map(|r| r["selected"].as_bool().unwrap()).collect();
        assert_eq!(selected, [true, true, true], "{json}");
        p.drag(8, 1.0, &[0.7, 0.4]);
        assert!(close(p.opacity(top), 0.4) && close(p.opacity(other), 0.4));
        assert!(close(p.opacity(background), 1.0) && p.state().doc.is_background(LayerId(background)), "the Background stays out");
        p.undo();
        assert!(close(p.opacity(top), 1.0) && close(p.opacity(other), 1.0), "one step");
        p.apply("blend", Phase::Commit, 9, EditValue::Choice("screen".into())).unwrap();
        assert_eq!((p.layer(top).blend, p.layer(other).blend, p.layer(background).blend), (BlendMode::Screen, BlendMode::Screen, BlendMode::Normal));
    }

    #[test]
    fn blend_visibility_rename_and_select_apply() {
        let mut p = Inspector::new();
        let top = p.top;
        p.apply("blend", Phase::Commit, 1, EditValue::Choice("multiply".into())).unwrap();
        p.apply("rows", Phase::Commit, 2, EditValue::SetVisible { row: top.into(), visible: false }).unwrap();
        p.apply("rows", Phase::Commit, 3, EditValue::Rename { row: top.into(), name: "Sky".into() }).unwrap();
        let l = p.layer(top);
        assert_eq!((l.blend, l.visible, l.name.as_str()), (BlendMode::Multiply, false, "Sky"));
        let bottom = p.background();
        p.apply("rows", Phase::Commit, 4, EditValue::Select { row: bottom.into(), mode: SelectMode::Replace }).unwrap();
        assert_eq!(p.state().active_layer, Some(LayerId(bottom)));
    }

    fn select_row(p: &mut Inspector, row: u64, mode: SelectMode) {
        p.apply("rows", Phase::Commit, 1, EditValue::Select { row: row.into(), mode }).unwrap();
    }

    fn selected(p: &Inspector) -> Vec<u64> {
        let mut ids: Vec<u64> = p.state().selected_layers().iter().map(|id| id.0).collect();
        ids.sort_unstable();
        ids
    }

    /// The modes are the desktop Layers panel's clicks: plain, Command and Shift.
    #[test]
    fn rows_select_by_mode() {
        let mut p = Inspector::new();
        let (top, background) = (p.top, p.background());
        let (a, b) = (p.new_layer(), p.new_layer());
        select_row(&mut p, top, SelectMode::Replace);
        assert_eq!(selected(&p), [top]);
        select_row(&mut p, b, SelectMode::Toggle);
        assert_eq!(selected(&p), [top, b], "added");
        assert_eq!(p.state().active_layer, Some(LayerId(b)));
        select_row(&mut p, b, SelectMode::Toggle);
        assert_eq!(selected(&p), [top], "taken out again");
        select_row(&mut p, b, SelectMode::Range);
        assert_eq!(selected(&p), [top, a, b], "from the last selected to this one");
        select_row(&mut p, background, SelectMode::Replace);
        assert_eq!(selected(&p), [background]);
    }

    /// A row dragged out of a multi-selection moves the whole selection, in one step, as the
    /// desktop's drag does; an unselected row moves alone.
    #[test]
    fn a_selected_row_moves_the_whole_selection() {
        let mut p = Inspector::new();
        let (top, background) = (p.top, p.background());
        let (a, b, c) = (p.new_layer(), p.new_layer(), p.new_layer());
        select_row(&mut p, top, SelectMode::Replace);
        select_row(&mut p, a, SelectMode::Toggle);
        let order = |p: &Inspector| p.state().doc.layers.iter().map(|l| l.id.0).collect::<Vec<_>>();
        assert_eq!(order(&p), [background, top, a, b, c], "bottom first");
        p.apply("rows", Phase::Commit, 2, EditValue::Move { row: top.into(), target: c.into(), position: Position::Above }).unwrap();
        assert_eq!(order(&p), [background, b, c, top, a], "both, in their order");
        p.undo();
        assert_eq!(order(&p), [background, top, a, b, c], "one step");
        p.apply("rows", Phase::Commit, 3, EditValue::Move { row: b.into(), target: c.into(), position: Position::Above }).unwrap();
        assert_eq!(order(&p), [background, top, a, c, b], "an unselected row alone");
    }

    #[test]
    fn a_row_moves_below_another() {
        let mut p = Inspector::new();
        let top = p.top;
        let third = p.new_layer();
        p.apply("rows", Phase::Commit, 1, EditValue::Move { row: third.into(), target: top.into(), position: Position::Below }).unwrap();
        let order: Vec<u64> = p.state().doc.layers.iter().map(|l| l.id.0).collect();
        assert_eq!(order, [p.background(), third, top], "bottom first");
    }

    /// Every row offers Duplicate and Delete (destructive), and the row's action acts on that layer
    /// alone, in one undo step each, whatever is selected.
    #[test]
    fn rows_duplicate_and_delete_their_layer() {
        let mut p = Inspector::new();
        let (top, background) = (p.top, p.background());
        let json = p.json();
        for row in json["nodes"][2]["rows"].as_array().unwrap() {
            assert_eq!(
                row["actions"].as_array().map(|a| a.iter().map(|x| (x["id"].clone(), x["destructive"].clone())).collect::<Vec<_>>()),
                Some(vec![(json!("duplicate"), json!(false)), (json!("delete"), json!(true))]),
                "{row}"
            );
        }
        let action = |action: &str| EditValue::Action { row: top.into(), action: action.into() };
        let count = |p: &Inspector| p.state().doc.layers.len();
        p.select(background, "replace");
        p.apply("rows", Phase::Commit, 1, action("duplicate")).unwrap();
        assert_eq!(count(&p), 3, "a copy of the row's layer");
        assert!(p.state().doc.layers.iter().any(|l| l.id.0 == top), "the original stays");
        p.undo();
        assert_eq!(count(&p), 2, "one step");
        p.select(background, "replace");
        p.apply("rows", Phase::Commit, 2, action("delete")).unwrap();
        assert_eq!(p.state().doc.layers.iter().map(|l| l.id.0).collect::<Vec<_>>(), [background], "the row's layer, not the selected one");
        p.undo();
        assert_eq!(count(&p), 2, "one step");
        assert!(p.apply("rows", Phase::Commit, 3, action("explode")).is_err());
    }

    #[test]
    fn a_rejected_edit_changes_nothing() {
        let mut p = Inspector::new();
        let before = p.state().revision;
        assert!(p.apply("blend", Phase::Commit, 1, EditValue::Choice("nonsense".into())).is_err());
        assert!(p.apply("blend", Phase::Commit, 4, EditValue::Choice("Multiply".into())).is_err(), "a label, not an id");
        assert!(p.apply("opacity", Phase::Commit, 2, EditValue::Toggle(true)).is_err());
        assert!(p.apply("opacity", Phase::Commit, 3, EditValue::Number(f64::NAN)).is_err());
        assert_eq!(p.state().revision, before);
    }
}
