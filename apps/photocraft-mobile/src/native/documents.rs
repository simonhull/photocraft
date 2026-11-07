//! The Documents inspector: the open documents, in tab order, the active one selected. It stands
//! in for PhotoCraft's document tabs while the shell draws the panels.
//!
//! A row is a document by its id, so a row stays its document while others open and close; a tap
//! makes it active, as Window › <document> does, and its Close (the row's context menu, or a
//! swipe) closes it, as its tab's Close does: a document with unsaved changes asks first.

use craft_mobile::ui::{Edit, EditValue, ItemList, Node, Panel, PanelRole, Row, RowAction, RowId};
use photocraft_ui_egui::PhotocraftApp;
use serde_json::json;

/// The document rows.
const ROWS: &str = "documents";
/// A row's Close.
const CLOSE: &str = "close";

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
            Row::new(st.doc.id.0, st.doc.name.as_str())
                .subtitle(subtitle)
                .selected(active == Some(i))
                .actions(vec![RowAction::new(CLOSE, "Close").icon(Some("xmark")).destructive(true)])
        })
        .collect();
    Panel::new(super::DOCUMENTS, "Documents", PanelRole::Inspector, vec![Node::ItemList(ItemList::new(ROWS, rows))])
}

/// Make the tapped document active, or close the row's document (through PhotoCraft's
/// unsaved-changes prompt, as its tab's Close).
pub(super) fn apply(app: &mut PhotocraftApp, ctx: &egui::Context, edit: &Edit) -> Result<(), String> {
    let index = |app: &PhotocraftApp, row: RowId| {
        app.session.documents().iter().position(|st| st.doc.id.0 == row.get()).ok_or_else(|| format!("documents: no document {row}"))
    };
    match (&edit.value, edit.control.as_str()) {
        (EditValue::Select { row, .. }, ROWS) => {
            let i = index(app, *row)?;
            photocraft_ui_egui::menus::invoke(app, ctx, &format!("window.document.{i}"), json!({})).map(drop)
        }
        (EditValue::Action { row, action }, ROWS) if action.as_str() == CLOSE => {
            let i = index(app, *row)?;
            photocraft_ui_egui::menus::invoke(app, ctx, "file.close", json!({"document": i})).map(drop)
        }
        (value, control) => Err(format!("documents: `{control}` doesn't take {value:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_mobile::ui::{GestureId, Phase, SelectMode};
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
        for row in &rows {
            assert_eq!(row["actions"], json!([{"id": "close", "label": "Close", "icon": "xmark", "destructive": true}]), "each can be closed");
        }
    }

    /// A row's Close is File › Close for its document: a saved one closes, one with unsaved
    /// changes asks first (PhotoCraft's prompt), so it's still open.
    #[test]
    fn close_closes_the_rows_document_and_asks_first_when_it_has_unsaved_changes() {
        let (mut app, ctx) = (app(), egui::Context::default());
        let close = |row: u64| edit(EditValue::Action { row: RowId::from(row), action: "close".into() });
        let ids = |app: &PhotocraftApp| app.session.documents().iter().map(|st| st.doc.id.0).collect::<Vec<_>>();
        let (first, second) = (ids(&app)[0], ids(&app)[1]);
        app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
        apply(&mut app, &ctx, &close(second)).unwrap();
        assert_eq!(ids(&app), [first, second], "unsaved changes: the prompt asks, nothing is closed yet");
        apply(&mut app, &ctx, &close(999)).unwrap_err();
        apply(&mut app, &ctx, &edit(EditValue::Action { row: RowId::from(first), action: "explode".into() })).unwrap_err();
    }

    #[test]
    fn close_closes_a_saved_document() {
        let (mut app, ctx) = (app(), egui::Context::default());
        let first = app.session.documents()[0].doc.id.0;
        apply(&mut app, &ctx, &edit(EditValue::Action { row: RowId::from(first), action: "close".into() })).unwrap();
        assert_eq!(app.session.documents().len(), 1);
        assert!(app.session.documents().iter().all(|st| st.doc.id.0 != first), "that one");
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
