//! Whether the transport bar shows: on demand, from Window › Timeline, and per document.
//!
//! The bar is hidden until Window › Timeline shows it, even for a document with a timeline; the
//! item shows or hides it again and is checked while it shows. Each document keeps its own choice
//! for the session, so switching documents brings each one's bar back as it was. A document
//! opened with a timeline starts as the last choice made, which is kept across launches
//! ([`SETTING`]); one that gets a timeline here shows it, as the bar stands for what was just made.
//!
//! Without a timeline Window › Timeline stays PhotoCraft's, as on the desktop: it opens the
//! Timeline panel, to create one.

use std::collections::HashMap;

use photocraft_doc::DocId;
use photocraft_ui_egui::{PanelId, PhotocraftApp};

/// Window › Timeline.
pub(crate) const COMMAND: &str = "window.panel.timeline";

/// The settings key for the last choice: `true` while documents opened with a timeline start with
/// the bar shown.
pub(crate) const SETTING: &str = "photocraft-mobile.timeline-bar";

/// What became of a document's bar while it was active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seen {
    /// The document had no timeline: one made since shows its bar.
    Bare,
    /// The document had a timeline, its bar shown or not: its own from then on.
    Shown(bool),
}

/// Whether each document's transport bar shows, for the session.
#[derive(Debug, Default)]
pub(crate) struct TimelineBar {
    docs: HashMap<DocId, Seen>,
    /// The last choice made: what a document opened with a timeline starts with.
    last: bool,
}

impl TimelineBar {
    /// Documents opened with a timeline start with the bar shown if `last` (the choice saved last
    /// time), hidden otherwise.
    pub(crate) fn new(last: bool) -> Self {
        Self { docs: HashMap::new(), last }
    }

    /// The last choice made, to save ([`SETTING`]).
    pub(crate) fn last(&self) -> bool {
        self.last
    }

    /// Whether the active document has a timeline and its bar shows.
    pub(crate) fn shows(&self, app: &PhotocraftApp) -> bool {
        active_timeline(app).is_some_and(|doc| self.shown(doc))
    }

    /// Once a frame: settle the active document's state, so a later choice for another document
    /// leaves it as it is, and note one without a timeline (a timeline made in it shows its bar).
    pub(crate) fn note(&mut self, app: &PhotocraftApp) {
        let Some(st) = app.session.active() else { return };
        let seen = if st.doc.timeline.is_some() { Seen::Shown(self.shown(st.doc.id)) } else { Seen::Bare };
        self.docs.insert(st.doc.id, seen);
    }

    /// Window › Timeline while the shell draws the bar and the active document has a timeline:
    /// show or hide the document's bar, which becomes the last choice. `Some` with whether it
    /// shows now; `None` otherwise, for PhotoCraft to run it.
    pub(crate) fn toggle(&mut self, app: &PhotocraftApp) -> Option<bool> {
        let doc = native_bar(app)?;
        let shown = !self.shown(doc);
        self.docs.insert(doc, Seen::Shown(shown));
        self.last = shown;
        Some(shown)
    }

    /// Window › Timeline's check mark while the shell draws the bar and the active document has a
    /// timeline: on while the bar shows. `None` for any other command, or otherwise.
    pub(crate) fn checked(&self, app: &PhotocraftApp, id: &str) -> Option<bool> {
        if id != COMMAND {
            return None;
        }
        native_bar(app).map(|doc| self.shown(doc))
    }

    /// Whether `doc`'s bar shows, if it has a timeline.
    fn shown(&self, doc: DocId) -> bool {
        match self.docs.get(&doc) {
            Some(Seen::Shown(shown)) => *shown,
            Some(Seen::Bare) => true,
            None => self.last,
        }
    }
}

/// The active document, if it has a timeline.
fn active_timeline(app: &PhotocraftApp) -> Option<DocId> {
    app.session.active().filter(|st| st.doc.timeline.is_some()).map(|st| st.doc.id)
}

/// The active document, if it has a timeline and the shell draws its bar (PhotoCraft's host chrome
/// lists the Timeline among the native panels).
fn native_bar(app: &PhotocraftApp) -> Option<DocId> {
    let drawn = app.host_chrome().is_some_and(|chrome| chrome.native_panels.contains(&PanelId::TIMELINE));
    drawn.then(|| active_timeline(app)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_engine::Session;
    use photocraft_ui_egui::{Drawn, HostChrome, Services};
    use serde_json::json;

    /// An app whose host draws the Timeline's bar, with `docs` documents (the last active).
    fn app(docs: usize) -> PhotocraftApp {
        let mut app = PhotocraftApp::new(Session::new(), Services::default());
        app.set_host_chrome(Some(HostChrome { dock: Drawn::ByHost, native_panels: vec![PanelId::TIMELINE], ..HostChrome::default() }));
        for _ in 0..docs {
            app.run("file.new", json!({"width": 8, "height": 8})).unwrap();
        }
        app
    }

    fn add_timeline(app: &mut PhotocraftApp) {
        app.run("timeline.create", json!({"duration": 10, "fps": 4.0})).unwrap();
    }

    #[test]
    fn hidden_until_asked_for_even_with_a_timeline() {
        let (mut app, mut bar) = (app(1), TimelineBar::default());
        add_timeline(&mut app);
        assert!(!bar.shows(&app));
        assert_eq!(bar.checked(&app, COMMAND), Some(false));
        assert_eq!(bar.toggle(&app), Some(true));
        assert!(bar.shows(&app) && bar.last());
        assert_eq!(bar.checked(&app, COMMAND), Some(true));
        assert_eq!(bar.toggle(&app), Some(false));
        assert!(!bar.shows(&app) && !bar.last());
        assert_eq!(bar.checked(&app, "window.panel.layers"), None, "only Window › Timeline");
    }

    /// Without a timeline, or with a host that has no bar, Window › Timeline is PhotoCraft's.
    #[test]
    fn photocraft_runs_window_timeline_where_there_is_no_bar() {
        let (mut app, mut bar) = (app(1), TimelineBar::default());
        assert_eq!((bar.toggle(&app), bar.checked(&app, COMMAND)), (None, None), "no timeline: the panel, to create one");
        add_timeline(&mut app);
        app.set_host_chrome(None);
        assert_eq!((bar.toggle(&app), bar.checked(&app, COMMAND)), (None, None), "no bar: PhotoCraft's panel");
        assert!(!bar.shows(&app));
    }

    /// Each document keeps its own choice; one opened since starts as the last choice made, one
    /// seen before keeps its own whatever was chosen for another since.
    #[test]
    fn each_document_keeps_its_own_choice() {
        let (mut app, mut bar) = (app(3), TimelineBar::default());
        for i in 0..3 {
            app.session.set_active(i);
            add_timeline(&mut app);
        }
        app.session.set_active(0);
        bar.note(&app);
        app.session.set_active(1);
        bar.toggle(&app);
        app.session.set_active(0);
        assert!(!bar.shows(&app), "seen hidden before the choice for the other: still hidden");
        app.session.set_active(2);
        assert!(bar.shows(&app), "not seen yet: the last choice");
        bar.toggle(&app);
        app.session.set_active(1);
        assert!(bar.shows(&app), "its own choice, whatever was chosen since");
    }

    /// A timeline made in a document shows its bar, whatever the last choice; one deleted and made
    /// again shows it again.
    #[test]
    fn a_timeline_made_here_shows_its_bar() {
        let (mut app, mut bar) = (app(1), TimelineBar::default());
        bar.note(&app);
        add_timeline(&mut app);
        assert!(bar.shows(&app));
        bar.note(&app);
        bar.toggle(&app);
        app.run("timeline.delete", json!({})).unwrap();
        bar.note(&app);
        add_timeline(&mut app);
        assert!(bar.shows(&app), "made again");
    }

    #[test]
    fn the_last_choice_carries_over() {
        let mut app = app(1);
        add_timeline(&mut app);
        assert!(TimelineBar::new(true).shows(&app));
        assert!(!TimelineBar::new(false).shows(&app));
    }
}
