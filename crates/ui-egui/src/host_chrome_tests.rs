//! Host chrome (`Services::host_chrome`): a part the host app draws itself is left out, and the
//! canvas takes its room; the default leaves everything to PhotoCraft.

use egui::Rect;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
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
        let services = crate::Services { host_chrome: chrome, ..Default::default() };
        let mut app = PhotocraftApp::new(s, services);
        app.ui.theme = ThemeKind::Pro;
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
    };
    let h = app(Some(all));
    let r = canvas(&h);
    assert!((r.min - egui::Pos2::ZERO).length() < 1.0 && (r.size() - SIZE).length() < 1.0, "{r:?}");
    for label in [TOOLBAR, OPTIONS, STATUS, DOCK] {
        assert!(!shows(&h, label), "{label} isn't drawn");
    }
}
