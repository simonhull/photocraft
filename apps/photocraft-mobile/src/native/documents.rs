//! The Documents inspector: the open documents, in tab order, the active one selected. It stands
//! in for PhotoCraft's document tabs while the shell draws the panels.
//!
//! A row is a document by its id, so a row stays its document while others open and close; a tap
//! makes it active, as Window › <document> does. (Closing one is File › Close: the kit's item
//! list has no row action for it yet.)

use craft_mobile::ui::{Edit, EditValue, ItemList, Node, Panel, PanelRole, Row};
use photocraft_ui_egui::PhotocraftApp;
use serde_json::json;

/// The document rows.
const ROWS: &str = "documents";

pub(super) fn describe(app: &PhotocraftApp) -> Panel {
    let active = app.session.active_index();
    let rows = app
        .session
        .documents()
        .iter()
        .enumerate()
        .map(|(i, st)| {
            let size = format!("{} × {} px", st.doc.size.width, st.doc.size.height);
            let subtitle = if st.is_dirty() { format!("Edited · {size}") } else { size };
            Row::new(st.doc.id.0, st.doc.name.as_str()).subtitle(subtitle).selected(active == Some(i))
        })
        .collect();
    Panel::new(super::DOCUMENTS, "Documents", PanelRole::Inspector, vec![Node::ItemList(ItemList::new(ROWS, rows))])
}

/// Make the tapped document active.
pub(super) fn apply(app: &mut PhotocraftApp, ctx: &egui::Context, edit: &Edit) -> Result<(), String> {
    match (&edit.value, edit.control.as_str()) {
        (EditValue::Select { row, .. }, ROWS) => {
            let index = app.session.documents().iter().position(|st| st.doc.id.0 == row.get()).ok_or_else(|| format!("documents: no document {row}"))?;
            photocraft_ui_egui::menus::invoke(app, ctx, &format!("window.document.{index}"), json!({})).map(drop)
        }
        (value, control) => Err(format!("documents: `{control}` doesn't take {value:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_mobile::ui::{GestureId, Phase, RowId, SelectMode};
    use photocraft_engine::Session;
    use photocraft_ui_egui::Services;
    use serde_json::Value;

    fn app() -> PhotocraftApp {
        let mut app = PhotocraftApp::new(Session::new(), Services::default());
        for (w, h) in [(8, 8), (16, 4)] {
            app.run("file.new", json!({"width": w, "height": h})).unwrap();
        }
        app
    }

    fn rows(app: &PhotocraftApp) -> Vec<Value> {
        let panel = describe(app);
        panel.check().unwrap();
        serde_json::to_value(panel).unwrap()["nodes"][0]["rows"].as_array().unwrap().clone()
    }

    fn edit(value: EditValue) -> Edit {
        Edit { panel: super::super::DOCUMENTS.into(), control: ROWS.into(), phase: Phase::Commit, gesture: GestureId::from(1), value }
    }

    #[test]
    fn the_open_documents_are_listed_in_tab_order_the_active_one_selected() {
        let mut app = app();
        app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
        let rows = rows(&app);
        let docs = app.session.documents();
        assert_eq!(rows.len(), 2);
        for (row, st) in rows.iter().zip(docs) {
            assert_eq!(row["id"], json!(st.doc.id.0));
            assert_eq!(row["title"], json!(st.doc.name));
        }
        assert_eq!(rows.iter().map(|r| r["selected"].as_bool().unwrap()).collect::<Vec<_>>(), [false, true], "the newest is active");
        assert_eq!(rows[0]["subtitle"], "8 × 8 px");
        assert_eq!(rows[1]["subtitle"], "Edited · 16 × 4 px", "unsaved changes say so");
    }

    #[test]
    fn selecting_a_row_makes_its_document_active() {
        let (mut app, ctx) = (app(), egui::Context::default());
        let first = app.session.documents()[0].doc.id.0;
        apply(&mut app, &ctx, &edit(EditValue::Select { row: RowId::from(first), mode: SelectMode::Replace })).unwrap();
        assert_eq!(app.session.active_index(), Some(0));
        assert!(apply(&mut app, &ctx, &edit(EditValue::Select { row: RowId::from(999), mode: SelectMode::Replace })).is_err());
        assert!(apply(&mut app, &ctx, &edit(EditValue::Rename { row: RowId::from(first), name: "x".into() })).is_err());
        assert_eq!(app.session.active_index(), Some(0));
    }
}
