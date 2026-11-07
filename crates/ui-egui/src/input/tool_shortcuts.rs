//! Toolbar tools in Edit › Keyboard Shortcuts (#4086). Explicit assignments select an
//! individual tool; unchanged tools keep Photoshop's shared keys and Shift cycling.

use crate::{PhotocraftApp, state::Tool};

/// The command that selects `tool` (`tools.select.brush`), as Edit › Keyboard Shortcuts lists it.
pub fn id(tool: Tool) -> String {
    let mut name = format!("{tool:?}");
    if let Some(first) = name.get_mut(..1) {
        first.make_ascii_lowercase();
    }
    format!("tools.select.{name}")
}

/// Selects `tool` as its [`id`] command does from a menu click, for a host's own tool picker:
/// refused while a dialog or prompt holds the app ([`crate::menus::invoke_from_host`]).
///
/// # Errors
/// The refusal, or the command's own, as a message.
pub fn select(app: &mut PhotocraftApp, ctx: &egui::Context, tool: Tool) -> Result<(), String> {
    crate::menus::invoke_from_host(app, ctx, &id(tool), serde_json::json!({})).map(drop)
}

/// The tool a [`id`] command selects.
pub fn tool(id: &str) -> Option<Tool> {
    let name = id.strip_prefix("tools.select.")?;
    let tool = Tool::from_name(name)?;
    (id == self::id(tool)).then_some(tool)
}

pub(crate) fn default(tool: Tool) -> Option<String> {
    (tool.key() != '\0').then(|| tool.key().to_string())
}

/// The key that selects `tool` now (`B`, or the user's own from Edit › Keyboard Shortcuts), if any.
pub fn shortcut(app: &PhotocraftApp, tool: Tool) -> Option<String> {
    crate::shortcuts::effective_shortcut(app, &id(tool), default(tool).as_deref())
}

pub(crate) fn uses_default(app: &PhotocraftApp, tool: Tool) -> bool {
    !app.session.prefs().shortcuts.contains_key(&id(tool))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `select` picks a tool as its `tools.select.*` command does; a tool that can't be picked
    /// now (no document) is refused the same way.
    #[test]
    fn select_picks_a_tool_as_its_command_does() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        let ctx = egui::Context::default();
        select(&mut app, &ctx, Tool::Lasso).unwrap();
        assert_eq!(app.ui.tool, Tool::Lasso);
        select(&mut app, &ctx, Tool::Brush).unwrap();
        assert_eq!(app.ui.tool, Tool::Brush);
    }

    /// Under the unsaved-changes prompt a host's tool picker is refused, as a menu click is.
    #[test]
    fn select_is_refused_under_a_prompt() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        let ctx = egui::Context::default();
        app.run("file.new", json!({"width": 8, "height": 8})).unwrap();
        app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
        crate::menus::invoke(&mut app, &ctx, "file.close", json!({})).unwrap();
        assert!(app.discard.is_some(), "the prompt is up");
        let before = app.ui.tool;
        let other = if before == Tool::Lasso { Tool::Brush } else { Tool::Lasso };
        assert!(select(&mut app, &ctx, other).is_err());
        assert_eq!(app.ui.tool, before);
    }

    #[test]
    fn every_toolbar_tool_has_a_unique_live_shortcut_row() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        let ctx = egui::Context::default();
        let items = crate::prefs_ui::shortcut_items(&app);
        let mut ids = std::collections::HashSet::new();
        for t in Tool::ALL {
            let id = id(t);
            assert!(ids.insert(id.clone()));
            assert_eq!(tool(&id), Some(t));
            assert!(crate::menus::is_live(&id));
            assert!(crate::menus::is_enabled(&app, &id));
            let rows: Vec<_> = items.iter().filter(|i| i.0 == id).collect();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].1, t.label());
            assert_eq!(rows[0].2, ["Tools"]);
            assert_eq!(rows[0].3, default(t));
            assert_eq!(crate::shortcuts::default_shortcut(&id), default(t));
            crate::menus::invoke(&mut app, &ctx, &id, json!({})).unwrap();
            assert_eq!(app.ui.tool, t);
        }
        for id in ["tools.select.", "tools.select.missing", "tools.select.Brush"] {
            assert!(!crate::menus::is_live(id));
            assert!(crate::menus::invoke(&mut app, &ctx, id, json!({})).is_err());
        }
    }

    #[test]
    fn toolbar_hints_follow_assignments_removal_and_reset() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        assert_eq!(shortcut(&app, Tool::Pencil).as_deref(), Some("B"));
        assert_eq!(shortcut(&app, Tool::SingleRowMarquee), None);
        app.run("prefs.set", json!({"path": "shortcuts.tools.select.pencil", "value": "F6"})).unwrap();
        assert_eq!(shortcut(&app, Tool::Pencil).as_deref(), Some("F6"));
        app.run("prefs.set", json!({"path": "shortcuts.tools.select.pencil", "value": ""})).unwrap();
        assert_eq!(shortcut(&app, Tool::Pencil), None);
        app.run("prefs.reset", json!({"path": "shortcuts.tools.select.pencil"})).unwrap();
        assert_eq!(shortcut(&app, Tool::Pencil).as_deref(), Some("B"));
    }

    #[test]
    fn a_remapped_tool_letter_does_not_steal_text_input() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("prefs.set", json!({"path": "shortcuts.tools.select.pencil", "value": "Q"})).unwrap();
        let mut h = egui_kittest::Harness::builder().build_ui_state(
            |ui, app: &mut PhotocraftApp| {
                let edit = ui.text_edit_singleline(&mut app.ui.status);
                edit.request_focus();
                edit.accessible_name("Status");
                crate::shortcuts::handle(app, ui.ctx());
            },
            app,
        );
        h.run_steps(3);
        assert!(h.ctx.text_edit_focused());
        h.key_press(egui::Key::Q);
        h.event(egui::Event::Text("q".into()));
        h.run_steps(2);
        assert_eq!(h.state().ui.tool, Tool::Move);
        assert_eq!(h.state().ui.status, "q");
    }
}

#[cfg(test)]
mod engine_table_tests {
    use super::*;

    /// The engine knows the tools as data (headless shortcut sets); it must match the toolbar.
    #[test]
    fn the_engines_tool_table_matches_the_tools() {
        let ours: Vec<(String, &str, Option<String>)> = Tool::ALL.into_iter().map(|t| (id(t), t.label(), default(t))).collect();
        let engine: Vec<(String, &str, Option<String>)> =
            photocraft_engine::shell_commands::TOOLS.iter().map(|(i, n, k)| (i.to_string(), *n, k.map(str::to_string))).collect();
        assert_eq!(ours, engine, "update photocraft_engine::shell_commands::TOOLS");
    }
}
