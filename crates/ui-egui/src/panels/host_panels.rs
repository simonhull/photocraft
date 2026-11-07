//! The dock's panels when the host app draws the dock (`HostChrome::dock`): a panel the host has
//! natively (`HostChrome::native_panels`) opens through `Services::open_host_panel`; any other
//! opens as a floating card over the canvas.
//!
//! A card stands for its dock group (Color | Swatches | …) and shows one of its panels: the
//! group's `ui.panels` flag says whether it's open and its `dock_tabs` entry which panel it shows,
//! so Window › Swatches turns the Color card to Swatches. Closing the card clears the flag.

use serde_json::Value;

use super::float_card::{CardSize, FloatCard, Header};
use super::panel_docking::{PANELS, body, normalize};
use crate::PhotocraftApp;
use crate::dock::Group;

/// A card's first size, and how far each further card steps down and in from the last.
const CARD_SIZE: egui::Vec2 = egui::vec2(320.0, 420.0);
const CARD_STEP: f32 = 28.0;

/// The `PANELS` id a `window.toggle.*` or `window.panel.*` command names, if any.
fn named_panel(id: &str) -> Option<&'static str> {
    id.strip_prefix("window.toggle.").or_else(|| id.strip_prefix("window.panel.")).and_then(normalize)
}

/// Whether the host draws panel `id` itself.
fn native(app: &PhotocraftApp, id: &str) -> bool {
    app.services.host_chrome.as_ref().is_some_and(|chrome| chrome.native_panels.iter().any(|native| native == id))
}

/// Panel `id`'s group and its tab there.
fn place(app: &PhotocraftApp, id: &str) -> Option<(Group, usize)> {
    let (_, label, group) = PANELS.iter().find(|p| p.0 == id)?;
    let tab = group.tabs(app.ui.theme.is_pro()).iter().position(|name| name == label)?;
    Some((*group, tab))
}

/// The tab `group`'s card shows.
fn selected_tab(app: &PhotocraftApp, group: Group) -> usize {
    let mut tabs = app.ui.dock_tabs;
    *group.tab_mut(&mut tabs)
}

/// Whether panel `id` shows in an open card.
fn floating(app: &PhotocraftApp, id: &str) -> bool {
    place(app, id).is_some_and(|(group, tab)| group.shown(&app.ui.panels) && selected_tab(app, group) == tab)
}

/// The panel `group`'s card shows: its selected tab, or its first panel the host doesn't draw.
/// `None` when the host draws all of them.
fn card_panel(app: &PhotocraftApp, group: Group) -> Option<&'static str> {
    let in_group = |p: &&(&'static str, &'static str, Group)| p.2 == group && !native(app, p.0);
    let selected = group.tabs(app.ui.theme.is_pro()).get(selected_tab(app, group)).copied();
    PANELS.iter().filter(in_group).find(|p| Some(p.1) == selected).or_else(|| PANELS.iter().find(in_group)).map(|p| p.0)
}

/// Runs a panel toggle (`window.toggle.<panel>`, `window.panel.<panel>`) while the host draws the
/// dock. A native panel opens through `Services::open_host_panel`, its flag left alone; any other
/// shows in its group's card, or closes it if it's showing there. `{"show": bool}` (or `"on"`)
/// asks for one or the other. `None` for any other command, or with the dock drawn by PhotoCraft.
pub(crate) fn command(app: &mut PhotocraftApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if !app.host_draws(|chrome| chrome.dock) {
        return None;
    }
    let panel = named_panel(id)?;
    let show = match params.get("show").or_else(|| params.get("on")) {
        None => None,
        Some(value) => match value.as_bool() {
            Some(show) => Some(show),
            None => return Some(Err("show must be a boolean".into())),
        },
    };
    if native(app, panel) {
        if show != Some(false)
            && let Some(open) = app.services.open_host_panel.as_mut()
        {
            open(panel);
        }
        return Some(Ok(Value::Null));
    }
    let (group, tab) = place(app, panel)?;
    let show = show.unwrap_or_else(|| !floating(app, panel));
    *group.shown_mut(&mut app.ui.panels) = show;
    if show {
        *group.tab_mut(&mut app.ui.dock_tabs) = tab;
    }
    Some(Ok(Value::Null))
}

/// A panel toggle's checkmark while the host draws the dock: on while the panel shows in a card,
/// and always off for a native panel (the host knows what it shows). `None` for any other
/// command, or with the dock drawn by PhotoCraft.
pub(crate) fn checked(app: &PhotocraftApp, id: &str) -> Option<bool> {
    if !app.host_draws(|chrome| chrome.dock) {
        return None;
    }
    let panel = named_panel(id)?;
    Some(!native(app, panel) && floating(app, panel))
}

/// Draws the open panels' cards while the host draws the dock, each stepped from the last; closing
/// one clears its group's flag.
pub(crate) fn floating_panels(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let canvas = app.last_canvas_rect;
    let open: Vec<(Group, &'static str)> =
        Group::ALL.into_iter().filter(|group| group.shown(&app.ui.panels)).filter_map(|group| Some((group, card_panel(app, group)?))).collect();
    for (step, (group, panel)) in (0u8..).zip(open) {
        let (Some((_, label, _)), Some((_, tab))) = (PANELS.iter().find(|p| p.0 == panel), place(app, panel)) else { continue };
        // The card shows the panel its group has selected, as the dock would.
        *group.tab_mut(&mut app.ui.dock_tabs) = tab;
        let inset = 16.0 + CARD_STEP * f32::from(step);
        let card = FloatCard {
            id: egui::Id::unique(("host-panel-card", group.key())),
            title: label,
            pos: egui::pos2((canvas.right() - CARD_SIZE.x - inset).max(canvas.left() + 8.0), canvas.top() + inset),
            size: CardSize::Resizable(CARD_SIZE),
            header: Header::Touch,
        };
        if card.show(app, ctx, |_, _| {}, |app, ui| body(app, ui, panel)) {
            *group.shown_mut(&mut app.ui.panels) = false;
        }
    }
}
