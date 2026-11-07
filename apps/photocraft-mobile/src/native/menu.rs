//! PhotoCraft's menus in the shell's native menu bar (craft-mobile's [`ui::MenuBar`]): the iPad
//! menu bar and the toolbar's menu.
//!
//! [`MobileMenu`] is PhotoCraft's [`native_menu::Backend`] for the shell, so the menus are kept
//! up to date and run exactly as the macOS menu bar's are ([`native_menu::run`] and
//! [`NativeMenu::raw_input`](photocraft_ui_egui::native_menu::NativeMenu::raw_input)). They come
//! as the in-window bar has them ([`MenuStyle::Plain`]): the iPad has no app menu, so About and
//! Preferences stay where PhotoCraft puts them.
//!
//! The shell sends a chosen item back as its command, or as its shortcut when it has one (so
//! PhotoCraft's own key rules decide, as for the key itself); the adapter queues either in
//! [`MenuEvents`], and the next frame drains them as [`Event::Click`] and [`Event::Key`].

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use craft_mobile::Platform;
use craft_mobile::ui::{self, Menu, MenuEntry, MenuRole, Shortcut};
use photocraft_ui_egui::menus::TOP_MENUS;
use photocraft_ui_egui::native_menu::{self, Backend, Chord, Event, MenuStyle, Node};

/// What the shell delivered for the menus since the last frame, oldest first: the adapter's
/// [`CraftApp::command`](craft_mobile::CraftApp::command) and
/// [`CraftApp::shortcut`](craft_mobile::CraftApp::shortcut) queue here, [`MobileMenu`] drains.
pub(crate) type MenuEvents = Rc<RefCell<Vec<Event>>>;

/// The shell's native menu bar, as PhotoCraft's menu backend.
pub(crate) struct MobileMenu {
    platform: Platform,
    events: MenuEvents,
}

impl MobileMenu {
    pub(crate) fn new(platform: Platform, events: MenuEvents) -> Self {
        Self { platform, events }
    }
}

impl Backend for MobileMenu {
    fn sync(&mut self, bar: &native_menu::MenuBar) {
        match menu_bar(bar) {
            // The platform sends it only when it changed.
            Ok(bar) => self.platform.set_menu(bar),
            Err(e) => log::error!("photocraft-mobile: the menus don't make a native menu bar: {e}"),
        }
    }

    fn drain(&mut self) -> Vec<Event> {
        self.events.take()
    }

    fn style(&self) -> MenuStyle {
        MenuStyle::Plain
    }
}

/// PhotoCraft's menus as the shell's. Items without a label are left out, and so is a command
/// listed again (the shell finds an item's state by its command); the system items a Mac layout
/// adds (`Node::Standard`) have no place here. The menus are [`TOP_MENUS`], in order (as
/// [`native_menu::plain_layout`] makes them), so File, Edit, View, Window and Help take the
/// system menus' places by position, whatever language their titles are in.
pub(crate) fn menu_bar(bar: &native_menu::MenuBar) -> Result<ui::MenuBar, ui::UiError> {
    let mut seen = HashSet::new();
    let menus = bar
        .menus
        .iter()
        .enumerate()
        .filter(|(_, m)| !m.title.trim().is_empty())
        .map(|(i, m)| Menu::new(m.title.as_str(), entries(&m.children, &mut seen)).role(TOP_MENUS.get(i).copied().and_then(role)))
        .collect();
    ui::MenuBar::new(menus)
}

/// The system menu one of PhotoCraft's top menus (by its untranslated name) takes the place of.
fn role(top: &str) -> Option<MenuRole> {
    match top {
        "File" => Some(MenuRole::File),
        "Edit" => Some(MenuRole::Edit),
        "View" => Some(MenuRole::View),
        "Window" => Some(MenuRole::Window),
        "Help" => Some(MenuRole::Help),
        _ => None,
    }
}

fn entries<'a>(nodes: &'a [Node], seen: &mut HashSet<&'a str>) -> Vec<MenuEntry> {
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node {
            Node::Item(it) if it.label.trim().is_empty() => {}
            Node::Item(it) if !seen.insert(it.id.as_str()) => log::debug!("photocraft-mobile: `{}` is in the menus twice; showing it once", it.id),
            Node::Item(it) => out.push(
                MenuEntry::command(it.id.as_str(), it.label.as_str())
                    .shortcut(it.shortcut.as_deref().and_then(shortcut))
                    .enabled(it.enabled)
                    .checked(it.checked)
                    .into(),
            ),
            Node::Separator => out.push(MenuEntry::Separator),
            Node::Submenu { label, children, .. } if !label.trim().is_empty() => out.push(MenuEntry::submenu(label.as_str(), entries(children, seen))),
            Node::Submenu { .. } | Node::Standard(_) => {}
        }
    }
    out
}

/// A menu item's shortcut for the shell, when it's one a menu takes: with ⌘ or ⌃, or a function
/// key ([`Chord::native_ok`]). A bare letter is a tool key, which the canvas gets straight away.
fn shortcut(text: &str) -> Option<Shortcut> {
    let chord = Chord::parse(text).filter(Chord::native_ok)?;
    Shortcut::parse(&chord.portable()).inspect_err(|e| log::debug!("photocraft-mobile: menu shortcut {text}: {e}")).ok()
}

/// A shortcut the shell delivered, as PhotoCraft's key chord.
pub(crate) fn chord(shortcut: &Shortcut) -> Option<Chord> {
    Chord::parse(&shortcut.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_engine::Session;
    use photocraft_ui_egui::{PhotocraftApp, Services, menus};
    use serde_json::{Value, json};

    fn app() -> PhotocraftApp {
        let mut app = PhotocraftApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"width": 8, "height": 8})).unwrap();
        app
    }

    fn converted(app: &PhotocraftApp) -> Value {
        let bar = native_menu::plain_layout(&menus::menu_items(app), photocraft_ui_egui::i18n::Lang::EN);
        serde_json::to_value(menu_bar(&bar).unwrap()).unwrap()
    }

    /// The command entry with `id`, anywhere in the bar.
    fn command<'a>(bar: &'a Value, id: &str) -> Option<&'a Value> {
        fn find<'a>(items: &'a Value, id: &str) -> Option<&'a Value> {
            items.as_array()?.iter().find_map(|i| if i["type"] == "command" && i["id"] == id { Some(i) } else { find(&i["items"], id) })
        }
        bar["menus"].as_array()?.iter().find_map(|m| find(&m["items"], id))
    }

    #[test]
    fn the_menus_convert_with_shortcuts_and_check_marks() {
        let mut app = app();
        app.ui.extras.rulers = true;
        let bar = converted(&app);
        let titles: Vec<&str> = bar["menus"].as_array().unwrap().iter().map(|m| m["title"].as_str().unwrap()).collect();
        assert_eq!(titles.first(), Some(&"File"), "no app menu: {titles:?}");
        assert!(titles.contains(&"Window"));
        assert_eq!(command(&bar, "file.open").unwrap()["shortcut"], "Cmd+O");
        assert_eq!(command(&bar, "edit.undo").unwrap()["shortcut"], "Cmd+Z");
        assert_eq!(command(&bar, "view.rulers").unwrap()["checked"], true, "{}", command(&bar, "view.rulers").unwrap());
        assert_eq!(command(&bar, "window.document.0").unwrap()["checked"], true, "the open document, checked");
        let roles: Vec<(&str, &Value)> = bar["menus"].as_array().unwrap().iter().map(|m| (m["title"].as_str().unwrap(), &m["role"])).collect();
        for (title, role) in roles {
            let expected = match title {
                "File" | "Edit" | "View" | "Window" | "Help" => json!(title.to_lowercase()),
                _ => Value::Null,
            };
            assert_eq!(*role, expected, "{title}");
        }
        let submenu = bar["menus"].as_array().unwrap().iter().flat_map(|m| m["items"].as_array().unwrap()).any(|i| i["type"] == "submenu");
        assert!(submenu, "submenus kept");
    }

    /// A tool key or another bare key is the canvas's: no menu takes it.
    #[test]
    fn only_command_keys_and_function_keys_are_menu_shortcuts() {
        assert_eq!(shortcut("Cmd+Shift+Z").map(|s| s.to_string()).as_deref(), Some("Cmd+Shift+Z"));
        assert_eq!(shortcut("Ctrl+Cmd+M").map(|s| s.to_string()).as_deref(), Some("Ctrl+Cmd+M"));
        assert_eq!(shortcut("F7").map(|s| s.to_string()).as_deref(), Some("F7"));
        assert_eq!(shortcut("Alt+Cmd++").map(|s| s.to_string()).as_deref(), Some("Cmd+Alt++"));
        assert!(shortcut("B").is_none());
        assert!(shortcut("Shift+Tab").is_none());
    }

    #[test]
    fn a_command_listed_twice_shows_once() {
        let item =
            |id: &str| native_menu::Node::Item(native_menu::Item { id: id.into(), label: id.into(), shortcut: None, enabled: true, checked: None, role: None });
        let bar = native_menu::MenuBar {
            menus: vec![native_menu::Menu {
                title: "Edit".into(),
                role: native_menu::MenuRole::Normal,
                children: vec![item("a"), native_menu::Node::Submenu { label: "More".into(), children: vec![item("a"), item("b")], role: None }, item("")],
            }],
        };
        let json = serde_json::to_value(menu_bar(&bar).unwrap()).unwrap();
        assert_eq!(json["menus"][0]["items"].as_array().unwrap().len(), 2, "{json}");
        assert_eq!(json["menus"][0]["items"][1]["items"].as_array().unwrap().len(), 1, "{json}");
    }

    /// The commands and shortcuts the shell delivered come back as the menu's clicks and key
    /// presses, oldest first, once.
    #[test]
    fn delivered_commands_and_shortcuts_drain_as_events() {
        let events = MenuEvents::default();
        let mut menu = MobileMenu::new(Platform::default(), events.clone());
        events.borrow_mut().push(Event::Click("image.duplicate".into()));
        let undo = chord(&"Cmd+Z".parse().unwrap()).unwrap();
        events.borrow_mut().push(Event::Key(undo.clone()));
        assert_eq!(menu.drain(), [Event::Click("image.duplicate".into()), Event::Key(undo)]);
        assert!(menu.drain().is_empty());
        assert_eq!(menu.style(), MenuStyle::Plain);
    }

    /// A translated menu keeps its role: it is found by its place among `TOP_MENUS`.
    #[test]
    fn roles_go_by_place_not_title() {
        let menus =
            TOP_MENUS.iter().map(|top| native_menu::Menu { title: format!("«{top}»"), role: native_menu::MenuRole::Normal, children: vec![] }).collect();
        let json = serde_json::to_value(menu_bar(&native_menu::MenuBar { menus }).unwrap()).unwrap();
        let roles: Vec<Value> = json["menus"].as_array().unwrap().iter().map(|m| m["role"].clone()).collect();
        assert_eq!(roles.first(), Some(&json!("file")));
        assert_eq!(roles.iter().filter(|r| !r.is_null()).count(), 5, "{roles:?}");
    }

    /// Named keys go to the shell and come back as the same key press, end to end.
    #[test]
    fn named_keys_round_trip_through_the_shell() {
        for keys in ["Cmd+ArrowUp", "Ctrl+Tab", "Cmd+Enter", "Cmd+Backspace", "Cmd+Escape", "F1", "Alt+Cmd+Delete"] {
            let sent = shortcut(keys).unwrap_or_else(|| panic!("{keys} goes to the shell"));
            let wire = serde_json::to_value(&sent).unwrap();
            let back: Shortcut = serde_json::from_value(wire.clone()).unwrap();
            let c = chord(&back).unwrap_or_else(|| panic!("{wire} comes back"));
            assert_eq!(c.key_event(true), Chord::parse(keys).unwrap().key_event(true), "{keys} → {wire}");
        }
    }

    #[test]
    fn delivered_shortcuts_are_the_chords_photocraft_binds() {
        for keys in ["Cmd+Z", "Ctrl+Cmd+M", "Cmd+Alt+Shift+N", "Cmd++", "F5", "Cmd+'"] {
            let delivered: Shortcut = keys.parse().unwrap();
            let c = chord(&delivered).unwrap();
            assert_eq!(c, Chord::parse(keys).unwrap(), "{keys}");
            assert!(c.key_event(true).is_some(), "{keys} is a key egui knows");
        }
    }
}
