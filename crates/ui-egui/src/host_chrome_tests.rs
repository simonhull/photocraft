//! Host chrome (`Services::host_chrome`): a part the host app draws itself is left out, and the
//! canvas takes its room; the default leaves everything to PhotoCraft.

use std::cell::RefCell;
use std::rc::Rc;

use egui::Rect;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;

use crate::theme::ThemeKind;
use crate::{Drawn, HostChrome, PhotocraftApp};

const SIZE: egui::Vec2 = egui::vec2(1200.0, 800.0);

/// The real app with a document open, every panel on, and `chrome` as its host chrome.
fn app(chrome: Option<HostChrome>) -> Harness<'static, PhotocraftApp> {
    let mut s = photocraft_engine::Session::new();
    s.execute("file.new", json!({"width": 8, "height": 8})).unwrap();
    s.edit_prefs(|p| p.general.auto_show_home_screen = false);
    let mut h = Harness::builder().with_size(SIZE).with_max_steps(64).build_eframe(move |cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, ThemeKind::Pro);
        let mut app = PhotocraftApp::new(s, crate::Services::default());
        app.ui.theme = ThemeKind::Pro;
        app.set_host_chrome(chrome);
        app
    });
    h.run_steps(6);
    h
}

/// A part of [`HostChrome`], for setting it.
type Part = fn(&mut HostChrome) -> &mut Drawn;

/// Only `part` drawn by the host.
fn host_draws(part: Part) -> HostChrome {
    let mut chrome = HostChrome::default();
    *part(&mut chrome) = Drawn::ByHost;
    chrome
}

fn canvas(h: &Harness<'static, PhotocraftApp>) -> Rect {
    h.state().last_canvas_rect
}

/// Whether a widget whose label holds `label` is drawn; the status bar's zoom field only by its
/// whole label, as the Zoom Tool's button starts with it.
fn shows(h: &Harness<'static, PhotocraftApp>, label: &str) -> bool {
    if label == STATUS { h.query_all_by_label(label).next().is_some() } else { h.query_all_by_label_contains(label).next().is_some() }
}

/// What each part draws that nothing else does: a tool button (not the active tool's, which the
/// options bar names too), the Move tool's Auto-Select, the status bar's zoom field and the dock's
/// New Layer button.
const TOOLBAR: &str = "Brush Tool";
const OPTIONS: &str = "Auto-Select";
const STATUS: &str = "Zoom";
const DOCK: &str = "Create a new layer";

#[test]
fn the_default_host_chrome_changes_nothing() {
    let without = app(None);
    let default = app(Some(HostChrome::default()));
    assert_eq!(canvas(&without), canvas(&default));
    for label in [TOOLBAR, OPTIONS, STATUS, DOCK] {
        assert!(shows(&without, label), "{label} is drawn");
        assert_eq!(shows(&without, label), shows(&default, label), "{label}");
    }
}

#[test]
fn each_host_drawn_part_is_left_out_and_the_canvas_takes_its_room() {
    let full = canvas(&app(None));
    let parts: [(&str, Part, Option<&str>); 6] = [
        ("menus", |c| &mut c.menus, None),
        ("toolbar", |c| &mut c.toolbar, Some(TOOLBAR)),
        ("options bar", |c| &mut c.options_bar, Some(OPTIONS)),
        ("status bar", |c| &mut c.status_bar, Some(STATUS)),
        ("tabs", |c| &mut c.tabs, None),
        ("dock", |c| &mut c.dock, Some(DOCK)),
    ];
    for (name, part, label) in parts {
        let h = app(Some(host_draws(part)));
        if let Some(label) = label {
            assert!(!shows(&h, label), "{name}: {label} isn't drawn");
        }
        let r = canvas(&h);
        assert!(full.contains_rect(r) || r.contains_rect(full), "{name}: {full:?} vs {r:?}");
        let grown = match name {
            "menus" | "options bar" | "tabs" => r.top() < full.top(),
            "toolbar" => r.left() < full.left(),
            "status bar" => r.bottom() > full.bottom(),
            _ => r.right() > full.right(),
        };
        assert!(grown, "{name}: the canvas takes its room ({full:?} -> {r:?})");
    }
}

#[test]
fn with_all_the_chrome_host_drawn_the_canvas_fills_the_window() {
    let all = HostChrome {
        menus: Drawn::ByHost,
        toolbar: Drawn::ByHost,
        options_bar: Drawn::ByHost,
        status_bar: Drawn::ByHost,
        tabs: Drawn::ByHost,
        dock: Drawn::ByHost,
        native_panels: Vec::new(),
    };
    let h = app(Some(all));
    let r = canvas(&h);
    assert!((r.min - egui::Pos2::ZERO).length() < 1.0 && (r.size() - SIZE).length() < 1.0, "{r:?}");
    for label in [TOOLBAR, OPTIONS, STATUS, DOCK] {
        assert!(!shows(&h, label), "{label} isn't drawn");
    }
}

// ------------------------------------------------------------------ host panels and floating cards

/// Every part host-drawn, so the only cards and Close buttons are the floating panels'.
fn all_host_drawn() -> HostChrome {
    HostChrome {
        menus: Drawn::ByHost,
        toolbar: Drawn::ByHost,
        options_bar: Drawn::ByHost,
        status_bar: Drawn::ByHost,
        tabs: Drawn::ByHost,
        dock: Drawn::ByHost,
        native_panels: vec!["layers".into()],
    }
}

/// The app with all the chrome host-drawn and Layers native, recording the panels the host is
/// asked to open; no panel open to start with.
fn host_app() -> (Harness<'static, PhotocraftApp>, Rc<RefCell<Vec<String>>>) {
    let opened: Rc<RefCell<Vec<String>>> = Rc::default();
    let record = opened.clone();
    let mut h = app(Some(all_host_drawn()));
    h.state_mut().services.open_host_panel = Some(Box::new(move |id| record.borrow_mut().push(id.into())));
    (h, opened)
}

fn run(h: &mut Harness<'static, PhotocraftApp>, id: &str) {
    let ctx = h.ctx.clone();
    crate::menus::invoke(h.state_mut(), &ctx, id, json!({})).unwrap();
    h.run_steps(4);
}

/// The titles of the open cards: each card has a Close button, and its title is the label on its row.
fn cards(h: &Harness<'static, PhotocraftApp>) -> Vec<String> {
    let closes: Vec<Rect> = h.query_all_by_label("Close").map(|node| node.rect()).collect();
    let mut titles: Vec<String> = closes
        .iter()
        .filter_map(|close| {
            h.query_all_by_role(egui::accesskit::Role::Label)
                .filter(|node| (close.top()..=close.bottom()).contains(&node.rect().center().y) && node.rect().right() < close.left())
                .find_map(|node| {
                    let a = node.accesskit_node();
                    a.label().or_else(|| a.value()).map(|text| text.to_string())
                })
        })
        .collect();
    titles.sort();
    titles
}

fn menu_checked(h: &Harness<'static, PhotocraftApp>, id: &str) -> Option<bool> {
    crate::menus::menu_items(h.state()).into_iter().find(|item| item.id == id).and_then(|item| item.checked)
}

#[test]
fn setting_a_host_drawn_dock_closes_the_panels() {
    let (h, _) = host_app();
    assert!(cards(&h).is_empty(), "{:?}", cards(&h));
    assert!(!h.state().ui.panels.color && !h.state().ui.panels.properties);
}

#[test]
fn window_color_floats_a_color_card_and_toggling_it_again_closes_it() {
    let (mut h, _) = host_app();
    run(&mut h, "window.toggle.color");
    assert_eq!(cards(&h), ["Color"]);
    assert!(h.state().ui.panels.color);
    run(&mut h, "window.toggle.color");
    assert!(cards(&h).is_empty(), "{:?}", cards(&h));
    assert!(!h.state().ui.panels.color);
}

#[test]
fn closing_the_card_clears_its_flag() {
    let (mut h, _) = host_app();
    run(&mut h, "window.toggle.color");
    h.get_by_label("Close").click();
    h.run_steps(4);
    assert!(!h.state().ui.panels.color);
    assert!(cards(&h).is_empty(), "{:?}", cards(&h));
}

#[test]
fn a_panel_of_an_open_card_turns_the_card_to_it() {
    let (mut h, _) = host_app();
    run(&mut h, "window.toggle.color");
    run(&mut h, "window.panel.swatches");
    assert_eq!(cards(&h), ["Swatches"]);
    run(&mut h, "window.toggle.history");
    assert_eq!(cards(&h), ["History", "Swatches"]);
}

#[test]
fn a_native_panel_opens_through_the_host_and_leaves_its_flag_alone() {
    let (mut h, opened) = host_app();
    run(&mut h, "window.toggle.layers");
    run(&mut h, "window.panel.layers");
    assert_eq!(*opened.borrow(), ["layers", "layers"]);
    assert!(!h.state().ui.panels.layers);
    assert!(cards(&h).is_empty(), "{:?}", cards(&h));
    // Channels shares the Layers group but isn't native: it floats, in a card of its own.
    run(&mut h, "window.panel.channels");
    assert_eq!(cards(&h), ["Channels"]);
    assert_eq!(opened.borrow().len(), 2);
}

#[test]
fn floating_panels_are_checked_and_native_ones_never() {
    let (mut h, _) = host_app();
    assert_eq!(menu_checked(&h, "window.panel.color"), Some(false));
    run(&mut h, "window.toggle.color");
    assert_eq!(menu_checked(&h, "window.panel.color"), Some(true));
    assert_eq!(menu_checked(&h, "window.panel.swatches"), Some(false), "the card shows Color, not Swatches");
    run(&mut h, "window.toggle.layers");
    h.state_mut().ui.panels.layers = true;
    assert_eq!(menu_checked(&h, "window.panel.layers"), Some(false), "the host knows what it shows");
}
