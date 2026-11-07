//! The tool rail: PhotoCraft's toolbar as a native [`ToolPalette`], slot for slot (Edit ›
//! Toolbar's hidden tools left out), each tool with an SF Symbol and the key that selects it.
//!
//! Picking a tool runs its `tools.select.<tool>` command, as the toolbar does. The selected tool's
//! options: the brush popover for the painting tools it serves, and for every tool Window ›
//! Options (`window.toggle.options`), which shows PhotoCraft's options bar as a floating card.

use craft_mobile::ui::{self, Edit, EditValue, Icon, Node, Panel, PanelRole, Phase, Shortcut, ToolPalette, ToolSlot};
use photocraft_ui_egui::panels::toolbar_slots;
use photocraft_ui_egui::{PhotocraftApp, Tool, tool_shortcuts};

/// The palette's control id.
const PALETTE: &str = "palette";
/// Window › Options: the options bar as a floating card.
pub(crate) const OPTIONS: &str = "window.toggle.options";
/// The prefix of a tool's select command.
const SELECT: &str = "tools.select.";

pub(super) fn describe(app: &PhotocraftApp) -> Option<Panel> {
    let current = app.ui.tool;
    let mut shown = false;
    let mut slots: Vec<ToolSlot> = toolbar_slots(&app.session.prefs().toolbar.hidden)
        .iter()
        .map(|slot| {
            shown |= slot.tools().contains(&current);
            ToolSlot::new(slot.tools().iter().map(|&t| tool(app, t)).collect())
        })
        .collect();
    if !shown {
        // Hidden from the toolbar but in use (picked by its key): it shows, on its own.
        slots.push(ToolSlot::one(tool(app, current)));
    }
    let palette = ToolPalette::new(PALETTE, slots, id(current))
        .inspect_err(|e| log::error!("photocraft-mobile: the tool rail: {e}"))
        .ok()?
        .options(brush_family(current).then(|| super::BRUSH.into()))
        .fallback_options(Some(OPTIONS.into()));
    Some(Panel::new(super::TOOLS, "Tools", PanelRole::Rail, vec![Node::ToolPalette(palette)]))
}

/// Select the tool picked on the rail.
pub(super) fn apply(app: &mut PhotocraftApp, ctx: &egui::Context, edit: &Edit) -> Result<(), String> {
    match (edit.phase, &edit.value, edit.control.as_str()) {
        (Phase::Commit, EditValue::Choice(picked), PALETTE) => {
            let tool = tool_shortcuts::tool(&format!("{SELECT}{picked}")).ok_or_else(|| format!("tools: no tool `{picked}`"))?;
            tool_shortcuts::select(app, ctx, tool)
        }
        (_, value, control) => Err(format!("tools: `{control}` doesn't take {value:?}")),
    }
}

/// A tool's id on the rail: its select command's last part (`brush`, `rectMarquee`).
fn id(tool: Tool) -> String {
    let command = tool_shortcuts::id(tool);
    command.strip_prefix(SELECT).map_or_else(|| command.clone(), str::to_owned)
}

fn tool(app: &PhotocraftApp, tool: Tool) -> ui::Tool {
    let key = tool_shortcuts::shortcut(app, tool).and_then(|k| Shortcut::parse(&k).ok());
    ui::Tool::new(id(tool), tool.label(), Icon::symbol(symbol(tool)).or_letter(letter(tool))).shortcut(key)
}

/// The painting tools the brush popover (size, opacity, flow, Shape Dynamics) covers: the Brush
/// slot's and the Eraser, which paint with those settings. Every other tool's options are
/// PhotoCraft's own options bar.
fn brush_family(tool: Tool) -> bool {
    matches!(tool, Tool::Brush | Tool::Pencil | Tool::ColorReplacement | Tool::MixerBrush | Tool::Eraser)
}

/// The letter shown where the symbol doesn't exist: the tool's key, else its name's first letter.
fn letter(tool: Tool) -> char {
    match tool.key() {
        '\0' | ' ' => tool.label().chars().next().unwrap_or('?'),
        key => key,
    }
}

/// The tool's SF Symbol.
fn symbol(tool: Tool) -> &'static str {
    match tool {
        Tool::Move => "arrow.up.and.down.and.arrow.left.and.right",
        Tool::Artboard => "rectangle.on.rectangle",
        Tool::RectMarquee => "rectangle.dashed",
        Tool::EllipseMarquee => "circle.dashed",
        Tool::SingleRowMarquee => "arrow.left.and.right",
        Tool::SingleColumnMarquee => "arrow.up.and.down",
        Tool::Lasso => "lasso",
        Tool::PolygonLasso => "pentagon",
        Tool::MagneticLasso => "lasso.badge.sparkles",
        Tool::ObjectSelection => "square.dashed.inset.filled",
        Tool::QuickSelection => "scribble.variable",
        Tool::MagicWand => "wand.and.stars",
        Tool::Crop => "crop",
        Tool::PerspectiveCrop => "perspective",
        Tool::Slice => "square.split.2x2",
        Tool::SliceSelect => "square.split.2x2.fill",
        Tool::Frame => "photo.artframe",
        Tool::Eyedropper => "eyedropper",
        Tool::ColorSampler => "eyedropper.halffull",
        Tool::Ruler => "ruler",
        Tool::Note => "note.text",
        Tool::Count => "number",
        Tool::SpotHealing => "bandage",
        Tool::Remove => "wand.and.rays",
        Tool::Healing => "bandage.fill",
        Tool::Patch => "square.on.square.dashed",
        Tool::ContentAwareMove => "arrow.up.and.down.and.sparkles",
        Tool::RedEye => "eye",
        Tool::Brush => "paintbrush.pointed",
        Tool::Pencil => "pencil",
        Tool::ColorReplacement => "paintpalette",
        Tool::MixerBrush => "paintbrush",
        Tool::CloneStamp => "plus.square.on.square",
        Tool::PatternStamp => "square.grid.3x3.square",
        Tool::HistoryBrush => "clock.arrow.circlepath",
        Tool::ArtHistoryBrush => "clock",
        Tool::Eraser => "eraser",
        Tool::BackgroundEraser => "eraser.line.dashed",
        Tool::MagicEraser => "wand.and.rays.inverse",
        Tool::Gradient => "circle.lefthalf.striped.horizontal",
        Tool::PaintBucket => "drop.fill",
        Tool::Blur => "drop",
        Tool::Sharpen => "triangle",
        Tool::Smudge => "hand.point.up.left",
        Tool::Dodge => "sun.max",
        Tool::Burn => "flame",
        Tool::Sponge => "humidity",
        Tool::Pen => "pencil.tip",
        Tool::FreeformPen => "scribble",
        Tool::CurvaturePen => "point.topleft.down.to.point.bottomright.curvepath",
        Tool::AddAnchorPoint => "plus.circle",
        Tool::DeleteAnchorPoint => "minus.circle",
        Tool::ConvertPoint => "point.3.connected.trianglepath.dotted",
        Tool::Type => "textformat",
        Tool::VerticalType => "character.cursor.ibeam",
        Tool::HorizontalTypeMask => "textformat.characters.dottedunderline",
        Tool::VerticalTypeMask => "character.textbox",
        Tool::PathSelection => "cursorarrow",
        Tool::DirectSelection => "cursorarrow.rays",
        Tool::Rectangle => "rectangle",
        Tool::EllipseShape => "circle",
        Tool::Triangle => "triangle.fill",
        Tool::Polygon => "hexagon",
        Tool::Line => "line.diagonal",
        Tool::CustomShape => "star",
        Tool::Hand => "hand.raised",
        Tool::RotateView => "rotate.right",
        Tool::Zoom => "magnifyingglass",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_mobile::ui::GestureId;
    use photocraft_engine::Session;
    use photocraft_ui_egui::Services;
    use serde_json::{Value, json};

    fn app() -> PhotocraftApp {
        PhotocraftApp::new(Session::new(), Services::default())
    }

    fn palette(app: &PhotocraftApp) -> Value {
        let panel = describe(app).unwrap();
        panel.check().unwrap();
        serde_json::to_value(panel).unwrap()["nodes"][0].clone()
    }

    fn ids(palette: &Value) -> Vec<String> {
        palette["slots"].as_array().unwrap().iter().flat_map(|s| s["tools"].as_array().unwrap()).map(|t| t["id"].as_str().unwrap().to_owned()).collect()
    }

    fn pick(tool: &str) -> Edit {
        Edit {
            panel: super::super::TOOLS.into(),
            control: PALETTE.into(),
            phase: Phase::Commit,
            gesture: GestureId::from(1),
            value: EditValue::Choice(tool.into()),
        }
    }

    /// Slot for slot as the toolbar shows them, every visible tool once, the tool in use selected.
    #[test]
    fn the_rail_describes_every_visible_tool() {
        let app = app();
        let palette = palette(&app);
        let slots = toolbar_slots(&[]);
        let expected: Vec<String> = slots.iter().flat_map(|slot| slot.tools()).map(|t| id(*t)).collect();
        assert_eq!(ids(&palette), expected);
        assert_eq!(palette["slots"].as_array().unwrap().len(), slots.len());
        assert_eq!(palette["selected"], json!(id(app.ui.tool)));
        let brush = palette["slots"].as_array().unwrap().iter().flat_map(|s| s["tools"].as_array().unwrap()).find(|t| t["id"] == "brush").unwrap();
        assert_eq!(brush["label"], "Brush Tool");
        assert_eq!(brush["shortcut"], "B");
        assert_eq!(brush["icon"]["symbol"], "paintbrush.pointed");
    }

    #[test]
    fn hidden_tools_are_left_out_unless_in_use() {
        let mut app = app();
        app.session.edit_prefs(|p| p.toolbar.hidden = vec!["Sponge".into(), "Smudge".into()]);
        assert!(!ids(&palette(&app)).contains(&"sponge".to_owned()));
        app.ui.tool = Tool::Smudge;
        let palette = palette(&app);
        assert_eq!(ids(&palette).last().map(String::as_str), Some("smudge"), "in use: shown on its own");
        assert_eq!(palette["selected"], "smudge");
    }

    /// The symbol table covers every tool (the match is exhaustive), each with a fallback letter.
    #[test]
    fn every_tool_has_a_symbol_and_a_letter() {
        for tool in Tool::ALL {
            let symbol = symbol(tool);
            assert!(!symbol.is_empty() && symbol.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.'), "{tool:?}: {symbol}");
            assert!(letter(tool).is_alphabetic(), "{tool:?}");
        }
        assert_eq!(letter(Tool::Blur), 'B', "no key: the name's first letter");
        assert_eq!(letter(Tool::Move), 'V');
    }

    #[test]
    fn picking_a_tool_runs_its_select_command() {
        let (mut app, ctx) = (app(), egui::Context::default());
        apply(&mut app, &ctx, &pick("pencil")).unwrap();
        assert_eq!(app.ui.tool, Tool::Pencil);
        apply(&mut app, &ctx, &pick("rectMarquee")).unwrap();
        assert_eq!(app.ui.tool, Tool::RectMarquee);
        assert!(apply(&mut app, &ctx, &pick("telekinesis")).is_err());
        assert!(apply(&mut app, &ctx, &Edit { phase: Phase::Change, ..pick("brush") }).is_err(), "a palette sends commits only");
        assert_eq!(app.ui.tool, Tool::RectMarquee);
    }

    /// The brush popover for the painting tools it covers; Window › Options for every tool.
    #[test]
    fn options_are_the_brush_popover_or_the_options_bar() {
        let mut app = app();
        app.ui.tool = Tool::Brush;
        let p = palette(&app);
        assert_eq!((p["options"].clone(), p["fallback_options"].clone()), (json!("brush"), json!(OPTIONS)));
        app.ui.tool = Tool::Move;
        let p = palette(&app);
        assert_eq!((p["options"].clone(), p["fallback_options"].clone()), (Value::Null, json!(OPTIONS)));
    }
}
