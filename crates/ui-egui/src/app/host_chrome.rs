//! The parts of the chrome a host app draws itself (`HostChrome`, a tablet shell's native UI),
//! set only through [`PhotocraftApp::set_host_chrome`].
//!
//! The dock's panels and the options bar open on demand while the host draws them, as floating
//! cards. Their flags (`ui.panels`) then say which cards are open, so the host's takeover closes
//! them, and handing the parts back restores the flags they had. The preferences keep remembering
//! the flags from before (`dock::snapshot`), so host mode never changes a synced desktop's panels.

use std::borrow::Cow;

use crate::dock::Group;
use crate::state::Panels;
use crate::{Drawn, HostChrome, PhotocraftApp};

/// The flags of the parts that open on demand while host-drawn: the dock groups' and the
/// options bar's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OnDemand {
    /// In [`Group::ALL`]'s order.
    groups: [bool; Group::ALL.len()],
    options_bar: bool,
}

impl OnDemand {
    /// Every card closed.
    const CLOSED: Self = Self { groups: [false; Group::ALL.len()], options_bar: false };

    fn of(panels: &Panels) -> Self {
        Self { groups: Group::ALL.map(|group| group.shown(panels)), options_bar: panels.options_bar }
    }

    /// Sets these flags in `panels` for the parts `chrome` draws; the others stay as they are.
    fn apply(self, chrome: &HostChrome, panels: &mut Panels) {
        if chrome.dock == Drawn::ByHost {
            for (group, shown) in Group::ALL.into_iter().zip(self.groups) {
                *group.shown_mut(panels) = shown;
            }
        }
        if chrome.options_bar == Drawn::ByHost {
            panels.options_bar = self.options_bar;
        }
    }

    /// Whether any card of the parts `chrome` draws is open.
    fn any_open(chrome: &HostChrome, panels: &Panels) -> bool {
        let open = Self::of(panels);
        (chrome.dock == Drawn::ByHost && open.groups.contains(&true)) || (chrome.options_bar == Drawn::ByHost && open.options_bar)
    }
}

/// The host's chrome and the flags it set aside.
#[derive(Debug)]
pub(crate) struct HostState {
    chrome: HostChrome,
    /// The flags from before the takeover (or the last workspace applied since): restored when the
    /// host hands the parts back, and what the preferences remember meanwhile.
    before: OnDemand,
    /// What Tab hid, so a second Tab brings back just those cards.
    hidden_by_tab: Option<OnDemand>,
}

impl PhotocraftApp {
    /// The parts of the chrome the host app draws itself, if any.
    pub fn host_chrome(&self) -> Option<&HostChrome> {
        self.host.as_ref().map(|host| &host.chrome)
    }

    /// Hands parts of the chrome to the host app, or takes them back with `None`. The panels and
    /// options bar PhotoCraft showed in the parts the host takes over are closed (they open on
    /// demand from then on, as floating cards), and come back as they were when it gives them back.
    ///
    /// Idempotent: the chrome it already has changes nothing, so a host that says it again (a
    /// shell restarting) doesn't close the open cards.
    pub fn set_host_chrome(&mut self, chrome: Option<HostChrome>) {
        if self.host_chrome() == chrome.as_ref() {
            return;
        }
        if let Some(old) = self.host.take() {
            old.before.apply(&old.chrome, &mut self.ui.panels);
        }
        self.host = chrome.map(|chrome| {
            let before = OnDemand::of(&self.ui.panels);
            OnDemand::CLOSED.apply(&chrome, &mut self.ui.panels);
            HostState { chrome, before, hidden_by_tab: None }
        });
    }

    /// Whether the host app draws `part` of the chrome itself, so PhotoCraft leaves it out.
    pub(crate) fn host_draws(&self, part: fn(&HostChrome) -> Drawn) -> bool {
        self.host_chrome().is_some_and(|chrome| part(chrome) == Drawn::ByHost)
    }

    /// After a workspace was applied: its panels become the ones restored when the host hands the
    /// parts back, and their cards stay closed.
    pub(crate) fn host_closes_its_parts(&mut self) {
        if let Some(host) = self.host.as_mut() {
            host.before = OnDemand::of(&self.ui.panels);
            host.hidden_by_tab = None;
            OnDemand::CLOSED.apply(&host.chrome, &mut self.ui.panels);
        }
    }

    /// The panel flags the preferences remember: in host mode, the ones from before it for the
    /// parts the host draws.
    pub(crate) fn panels_to_remember(&self) -> Cow<'_, Panels> {
        let Some(host) = self.host.as_ref() else { return Cow::Borrowed(&self.ui.panels) };
        let mut panels = self.ui.panels.clone();
        host.before.apply(&host.chrome, &mut panels);
        Cow::Owned(panels)
    }

    /// Window › Hide Panels (Tab) in host mode: hides the open cards, and a second Tab brings back
    /// just those; with none open and none hidden it opens nothing. False outside host mode.
    pub(crate) fn host_toggle_cards(&mut self) -> bool {
        let Some(host) = self.host.as_mut() else { return false };
        let open = OnDemand::any_open(&host.chrome, &self.ui.panels);
        match host.hidden_by_tab.take() {
            Some(hidden) if !open => hidden.apply(&host.chrome, &mut self.ui.panels),
            _ if open => {
                host.hidden_by_tab = Some(OnDemand::of(&self.ui.panels));
                OnDemand::CLOSED.apply(&host.chrome, &mut self.ui.panels);
            }
            _ => {}
        }
        true
    }
}
