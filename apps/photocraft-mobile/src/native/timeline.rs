//! The transport bar: the active document's timeline (Window › Timeline), with Play, Stop,
//! stepping and a scrubber.
//!
//! Offered while the active document has a timeline, so the shell shows the bar whenever there is
//! a playhead to paint at. Every move of the playhead is an engine `timeline.*` command, run as a
//! menu click runs it (refused under a dialog); none is undoable, as on the desktop.
//!
//! Playing is the adapter's own state, as it is the desktop Timeline panel's: [`Player`] holds a
//! [`Playback`] clock and moves the playhead each frame ([`Player::tick`]), looping over the whole
//! duration (the desktop panel ignores the work area too). Any step, seek or jump pauses; Stop
//! pauses, and pressed again while paused goes back to the first frame.

use craft_mobile::ui::{Edit, EditValue, FrameRange, GestureId, Node, Panel, PanelRole, Phase, Playback, Rate, TimeFormat, Transport, TransportAction};
use photocraft_doc::{DocId, Timeline};
use photocraft_ui_egui::PhotocraftApp;
use serde_json::{Value, json};

/// The bar's one control.
const TRANSPORT: &str = "transport";

/// Playback in progress: of one document's timeline as it was when Play was pressed.
#[derive(Debug)]
struct Playing {
    doc: DocId,
    /// The timeline's length and rate at Play: a change to either stops playback.
    duration: u64,
    rate: Rate,
    clock: Playback,
}

impl Playing {
    /// Whether it still plays `timeline`: the same length and rate.
    fn plays(&self, timeline: &Timeline) -> bool {
        Frames::of(timeline).is_some_and(|f| f.duration == self.duration && f.rate == self.rate)
    }
}

/// A scrub in progress: the frame from before its `begin`, for a `cancel` to go back to.
#[derive(Debug)]
struct Scrub {
    gesture: GestureId,
    from: u64,
}

/// The transport's state between frames: what plays, and the scrub in progress.
#[derive(Debug, Default)]
pub(crate) struct Player {
    playing: Option<Playing>,
    scrub: Option<Scrub>,
}

impl Player {
    /// Whether it plays `doc`'s `timeline` as it is now.
    fn plays(&self, doc: DocId, timeline: &Timeline) -> bool {
        self.playing.as_ref().is_some_and(|p| p.doc == doc && p.plays(timeline))
    }

    /// Pause, the playhead where it is.
    pub(super) fn pause(&mut self) {
        self.playing = None;
    }

    /// Move the playhead to the frame due at `now` (seconds on a monotonic clock, the one
    /// [`apply`] was given), and ask for the next frame while playing; nothing while paused.
    ///
    /// Playback stops once the active document is another (or none), its timeline is gone or has
    /// another length or rate, or the move is refused (a dialog came up). A playhead moved from
    /// elsewhere meanwhile (a script, a shortcut) plays on from where it was put, as on the desktop.
    pub(super) fn tick(&mut self, app: &mut PhotocraftApp, ctx: &egui::Context, now: f64) {
        let Some(playing) = &mut self.playing else {
            return;
        };
        let current = app.session.active().filter(|st| st.doc.id == playing.doc).and_then(|st| st.doc.timeline.as_ref()).filter(|tl| playing.plays(tl));
        let Some(current) = current.and_then(|tl| u64::try_from(tl.current).ok()) else {
            self.playing = None;
            return;
        };
        if current != playing.clock.frame() {
            playing.clock.start(now, current);
        }
        if let Some(frame) = playing.clock.tick(now)
            && let Err(e) = set_frame(app, ctx, frame)
        {
            log::info!("photocraft-mobile: playback stopped: {e}");
            self.playing = None;
            return;
        }
        ctx.request_repaint();
    }
}

/// A timeline's frames as the transport counts them: never empty, the playhead inside, the work
/// area when it's narrower than the whole (a stored timeline may be out of bounds; the engine
/// clamps it on its next move).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Frames {
    duration: u64,
    current: u64,
    rate: Rate,
    work_area: Option<FrameRange>,
}

impl Frames {
    fn of(timeline: &Timeline) -> Option<Self> {
        let duration = u64::try_from(timeline.duration.max(1)).ok()?;
        let current = u64::try_from(timeline.current).ok()?.min(duration - 1);
        let rate = Rate::new(f64::from(timeline.fps)).ok()?;
        let (start, end) = (u64::try_from(timeline.work_start).ok()?, u64::try_from(timeline.work_end).ok()?.min(duration));
        // The whole duration shaded says nothing: no work area.
        let work_area = FrameRange::new(start, end).ok().filter(|r| r.frames().get() < duration);
        Some(Self { duration, current, rate, work_area })
    }
}

/// Whether the transport is offered: while the active document has a timeline.
pub(super) fn offered(app: &PhotocraftApp) -> bool {
    app.session.active().is_some_and(|st| st.doc.timeline.is_some())
}

/// The transport at the playhead; `None` without a timeline.
pub(super) fn describe(app: &PhotocraftApp, player: &Player) -> Option<Panel> {
    let st = app.session.active()?;
    let timeline = st.doc.timeline.as_ref()?;
    let f = Frames::of(timeline)?;
    let transport = Transport::new(TRANSPORT, f.current, f.duration, f.rate)
        .and_then(|t| t.range(f.work_area))
        .map_err(|e| log::warn!("photocraft-mobile: timeline: {e}"))
        .ok()?
        .format(TimeFormat::Frames)
        .playing(player.plays(st.doc.id, timeline));
    Some(Panel::new(super::TIMELINE, "Timeline", PanelRole::Transport, vec![Node::Transport(transport)]))
}

/// Apply a scrub or a button, `now` being the time on the clock [`Player::tick`] is given.
pub(super) fn apply(app: &mut PhotocraftApp, player: &mut Player, ctx: &egui::Context, edit: &Edit, now: f64) -> Result<(), String> {
    match (&edit.value, edit.control.as_str()) {
        (EditValue::Seek { frame }, TRANSPORT) => seek(app, player, ctx, edit, *frame),
        (EditValue::Transport(action), TRANSPORT) => press(app, player, ctx, *action, now),
        (value, control) => Err(format!("timeline: `{control}` doesn't take {value:?}")),
    }
}

/// Scrub: the playhead follows, paused. A `cancel` goes back to the frame from before the scrub.
fn seek(app: &mut PhotocraftApp, player: &mut Player, ctx: &egui::Context, edit: &Edit, frame: u64) -> Result<(), String> {
    player.pause();
    let frame = match edit.phase {
        Phase::Begin => {
            player.scrub = Some(Scrub { gesture: edit.gesture, from: frames(app)?.current });
            frame
        }
        Phase::Change => frame,
        Phase::Commit | Phase::Cancel => {
            let scrub = player.scrub.take_if(|s| s.gesture == edit.gesture);
            match (edit.phase, scrub) {
                (Phase::Cancel, Some(scrub)) => scrub.from,
                _ => frame,
            }
        }
    };
    set_frame(app, ctx, frame)
}

/// A button.
fn press(app: &mut PhotocraftApp, player: &mut Player, ctx: &egui::Context, action: TransportAction, now: f64) -> Result<(), String> {
    let was_playing = player.playing.take().is_some();
    match action {
        TransportAction::Play => {
            let (doc, f) =
                app.session.active().and_then(|st| Some((st.doc.id, Frames::of(st.doc.timeline.as_ref()?)?))).ok_or("timeline: no timeline to play")?;
            // From the playhead: run as a move to it, so Play is refused (under a dialog) where
            // the moves it makes would be.
            set_frame(app, ctx, f.current)?;
            let span = FrameRange::new(0, f.duration).map_err(|e| e.to_string())?;
            let mut clock = Playback::new(f.rate, span);
            clock.start(now, f.current);
            player.playing = Some(Playing { doc, duration: f.duration, rate: f.rate, clock });
            Ok(())
        }
        TransportAction::Pause => Ok(()),
        // The desktop rule: Stop pauses; pressed while paused, it goes back to the first frame.
        TransportAction::Stop if was_playing => Ok(()),
        TransportAction::Stop | TransportAction::ToStart => set_frame(app, ctx, 0),
        TransportAction::ToEnd => {
            let last = frames(app)?.duration - 1;
            set_frame(app, ctx, last)
        }
        TransportAction::StepBack => run(app, ctx, "timeline.previousFrame", json!({})),
        TransportAction::StepForward => run(app, ctx, "timeline.nextFrame", json!({})),
    }
}

/// The active document's timeline's frames.
fn frames(app: &PhotocraftApp) -> Result<Frames, String> {
    app.session.active().and_then(|st| Frames::of(st.doc.timeline.as_ref()?)).ok_or_else(|| "timeline: no timeline".to_owned())
}

/// Move the playhead to `frame` (the engine clamps it to the timeline).
fn set_frame(app: &mut PhotocraftApp, ctx: &egui::Context, frame: u64) -> Result<(), String> {
    run(app, ctx, "timeline.setFrame", json!({"frame": frame}))
}

/// Run a timeline command as a menu click runs it. Not through [`super::run`]: a playhead move is
/// no history step, so there's nothing to coalesce.
fn run(app: &mut PhotocraftApp, ctx: &egui::Context, id: &str, params: Value) -> Result<(), String> {
    photocraft_ui_egui::menus::invoke_from_host(app, ctx, id, params).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_engine::Session;
    use photocraft_ui_egui::Services;

    /// A document with a 10-frame timeline at 4 fps (every frame's time exact in binary).
    fn app() -> (PhotocraftApp, egui::Context, Player) {
        let mut app = PhotocraftApp::new(Session::new(), Services::default());
        app.run("file.new", json!({"width": 8, "height": 8})).unwrap();
        app.run("timeline.create", json!({"duration": 10, "fps": 4.0})).unwrap();
        (app, egui::Context::default(), Player::default())
    }

    fn timeline(app: &PhotocraftApp) -> Timeline {
        app.session.active().unwrap().doc.timeline.clone().unwrap()
    }

    fn current(app: &PhotocraftApp) -> usize {
        timeline(app).current
    }

    fn described(app: &PhotocraftApp, player: &Player) -> Value {
        let panel = describe(app, player).unwrap();
        panel.check().unwrap();
        let json = serde_json::to_value(panel).unwrap();
        assert_eq!((&json["id"], &json["role"]), (&json!("timeline"), &json!("transport")));
        json["nodes"][0].clone()
    }

    fn playing(app: &PhotocraftApp, player: &Player) -> bool {
        described(app, player)["playing"].as_bool().unwrap()
    }

    fn edit(phase: Phase, gesture: u64, value: EditValue) -> Edit {
        Edit { panel: super::super::TIMELINE.into(), control: TRANSPORT.into(), phase, gesture: gesture.into(), value }
    }

    fn press_at(app: &mut PhotocraftApp, ctx: &egui::Context, player: &mut Player, action: TransportAction, now: f64) -> Result<(), String> {
        apply(app, player, ctx, &edit(Phase::Commit, 1, EditValue::Transport(action)), now)
    }

    fn press(app: &mut PhotocraftApp, ctx: &egui::Context, player: &mut Player, action: TransportAction) {
        press_at(app, ctx, player, action, 0.0).unwrap();
    }

    fn seek(app: &mut PhotocraftApp, ctx: &egui::Context, player: &mut Player, phase: Phase, gesture: u64, frame: u64) {
        apply(app, player, ctx, &edit(phase, gesture, EditValue::Seek { frame }), 0.0).unwrap();
    }

    #[test]
    fn no_timeline_no_transport() {
        let mut app = PhotocraftApp::new(Session::new(), Services::default());
        assert!(!offered(&app) && describe(&app, &Player::default()).is_none(), "no document");
        app.run("file.new", json!({"width": 8, "height": 8})).unwrap();
        assert!(!offered(&app) && describe(&app, &Player::default()).is_none(), "a document without a timeline");
        app.run("timeline.create", json!({"duration": 10, "fps": 4.0})).unwrap();
        assert!(offered(&app) && describe(&app, &Player::default()).is_some());
        app.run("timeline.delete", json!({})).unwrap();
        assert!(!offered(&app) && describe(&app, &Player::default()).is_none(), "deleted");
    }

    #[test]
    fn describes_the_playhead_length_rate_and_work_area() {
        let (mut app, _, player) = app();
        app.run("timeline.setFrame", json!({"frame": 3})).unwrap();
        let t = described(&app, &player);
        assert_eq!(
            t,
            json!({"type": "transport", "id": "transport", "position": 3, "duration": 10, "rate": 4.0, "format": "frames", "playing": false, "range": null, "enabled": true}),
            "the work area is the whole duration: none shaded"
        );
        app.run("timeline.setProps", json!({"workStart": 2, "workEnd": 8, "fps": 29.97})).unwrap();
        let t = described(&app, &player);
        assert_eq!(t["range"], json!([2, 8]), "a narrower work area is shaded");
        assert!((t["rate"].as_f64().unwrap() - 29.97).abs() < 1e-4, "{t}");
    }

    /// A stored timeline out of bounds still shows, inside them.
    #[test]
    fn an_out_of_bounds_timeline_shows_inside_its_bounds() {
        let (mut app, _, player) = app();
        let st = app.session.active_mut().unwrap();
        let doc = std::sync::Arc::make_mut(&mut st.doc);
        doc.timeline = Some(Timeline { fps: 4.0, duration: 5, current: 9, work_start: 3, work_end: 99 });
        let t = described(&app, &player);
        assert_eq!((&t["position"], &t["duration"], &t["range"]), (&json!(4), &json!(5), &json!([3, 5])));
    }

    #[test]
    fn steps_and_jumps_move_the_playhead_and_pause() {
        let (mut app, ctx, mut player) = app();
        for (action, frame) in [
            (TransportAction::StepForward, 1),
            (TransportAction::StepForward, 2),
            (TransportAction::StepBack, 1),
            (TransportAction::ToEnd, 9),
            (TransportAction::StepForward, 9),
            (TransportAction::ToStart, 0),
            (TransportAction::StepBack, 0),
        ] {
            press(&mut app, &ctx, &mut player, TransportAction::Play);
            assert!(playing(&app, &player));
            press(&mut app, &ctx, &mut player, action);
            assert_eq!(current(&app), frame, "{action:?}");
            assert!(!playing(&app, &player), "{action:?} pauses");
        }
    }

    #[test]
    fn play_plays_from_the_playhead_and_pause_holds_it() {
        let (mut app, ctx, mut player) = app();
        app.run("timeline.setFrame", json!({"frame": 4})).unwrap();
        press_at(&mut app, &ctx, &mut player, TransportAction::Play, 10.0).unwrap();
        assert!(playing(&app, &player));
        assert_eq!(current(&app), 4, "from the playhead");
        player.tick(&mut app, &ctx, 10.25);
        assert_eq!(current(&app), 5);
        press(&mut app, &ctx, &mut player, TransportAction::Pause);
        assert!(!playing(&app, &player));
        player.tick(&mut app, &ctx, 20.0);
        assert_eq!(current(&app), 5, "paused: held");
    }

    /// The desktop rule: Stop pauses where it is; pressed again, paused, it rewinds.
    #[test]
    fn stop_pauses_then_rewinds() {
        let (mut app, ctx, mut player) = app();
        app.run("timeline.setFrame", json!({"frame": 4})).unwrap();
        press(&mut app, &ctx, &mut player, TransportAction::Play);
        press(&mut app, &ctx, &mut player, TransportAction::Stop);
        assert_eq!((current(&app), playing(&app, &player)), (4, false), "the first Stop pauses");
        press(&mut app, &ctx, &mut player, TransportAction::Stop);
        assert_eq!((current(&app), playing(&app, &player)), (0, false), "the second rewinds");
    }

    #[test]
    fn a_scrub_moves_the_playhead_and_pauses() {
        let (mut app, ctx, mut player) = app();
        press(&mut app, &ctx, &mut player, TransportAction::Play);
        seek(&mut app, &ctx, &mut player, Phase::Begin, 7, 2);
        assert_eq!(current(&app), 2, "live from the begin");
        assert!(!playing(&app, &player), "a scrub pauses");
        seek(&mut app, &ctx, &mut player, Phase::Change, 7, 6);
        assert_eq!(current(&app), 6);
        seek(&mut app, &ctx, &mut player, Phase::Commit, 7, 8);
        assert_eq!(current(&app), 8);
        seek(&mut app, &ctx, &mut player, Phase::Commit, 8, 99);
        assert_eq!(current(&app), 9, "past the end (the timeline shrank meanwhile): the last frame");
    }

    #[test]
    fn a_cancelled_scrub_goes_back() {
        let (mut app, ctx, mut player) = app();
        app.run("timeline.setFrame", json!({"frame": 3})).unwrap();
        seek(&mut app, &ctx, &mut player, Phase::Begin, 7, 5);
        seek(&mut app, &ctx, &mut player, Phase::Change, 7, 8);
        seek(&mut app, &ctx, &mut player, Phase::Cancel, 7, 8);
        assert_eq!(current(&app), 3, "back to the frame from before the scrub");
        // A cancel without its begin (the shell restarted) goes where it says.
        seek(&mut app, &ctx, &mut player, Phase::Cancel, 9, 6);
        assert_eq!(current(&app), 6);
    }

    #[test]
    fn playback_keeps_time_catches_up_and_wraps() {
        let (mut app, ctx, mut player) = app();
        press_at(&mut app, &ctx, &mut player, TransportAction::Play, 10.0).unwrap();
        for (now, frame) in [(10.1, 0), (10.25, 1), (10.5, 2), (10.75, 3)] {
            player.tick(&mut app, &ctx, now);
            assert_eq!(current(&app), frame, "at {now}");
        }
        player.tick(&mut app, &ctx, 12.0);
        assert_eq!(current(&app), 8, "a long frame skips ahead: five frames in 1.25 s");
        player.tick(&mut app, &ctx, 12.5);
        assert_eq!(current(&app), 0, "past the last frame, round to the first");
        assert!(playing(&app, &player), "it loops");
    }

    #[test]
    fn frames_keep_coming_only_while_playing() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let (mut app, ctx, mut player) = app();
        // As the host hears of it: the next frame is due when the callback says.
        let woken = Arc::new(AtomicBool::new(false));
        let wake = woken.clone();
        ctx.set_request_repaint_callback(move |info| {
            if info.delay.is_zero() {
                wake.store(true, Ordering::Relaxed);
            }
        });
        let asked = |_: &egui::Context| woken.swap(false, Ordering::Relaxed);
        player.tick(&mut app, &ctx, 1.0);
        assert!(!asked(&ctx), "paused: no frames asked for");
        press_at(&mut app, &ctx, &mut player, TransportAction::Play, 1.0).unwrap();
        player.tick(&mut app, &ctx, 1.1);
        assert!(asked(&ctx), "playing: the next frame");
        press(&mut app, &ctx, &mut player, TransportAction::Pause);
        player.tick(&mut app, &ctx, 1.2);
        assert!(!asked(&ctx));
    }

    #[test]
    fn a_playhead_moved_elsewhere_plays_on_from_there() {
        let (mut app, ctx, mut player) = app();
        press_at(&mut app, &ctx, &mut player, TransportAction::Play, 0.0).unwrap();
        app.run("timeline.setFrame", json!({"frame": 6})).unwrap();
        player.tick(&mut app, &ctx, 0.125);
        assert_eq!(current(&app), 6);
        player.tick(&mut app, &ctx, 0.375);
        assert_eq!(current(&app), 7, "a frame after it was moved");
    }

    #[test]
    fn deleting_the_timeline_stops_playback() {
        let (mut app, ctx, mut player) = app();
        press(&mut app, &ctx, &mut player, TransportAction::Play);
        app.run("timeline.delete", json!({})).unwrap();
        player.tick(&mut app, &ctx, 1.0);
        assert!(player.playing.is_none());
        app.run("timeline.create", json!({"duration": 10, "fps": 4.0})).unwrap();
        player.tick(&mut app, &ctx, 2.0);
        assert!(!playing(&app, &player), "a new timeline starts paused");
        assert_eq!(current(&app), 0);
    }

    /// A new length or rate (Timeline Settings) stops playback: Play again plays the new one.
    #[test]
    fn a_new_length_or_rate_stops_playback() {
        for props in [json!({"duration": 20}), json!({"fps": 12.0})] {
            let (mut app, ctx, mut player) = app();
            press(&mut app, &ctx, &mut player, TransportAction::Play);
            app.run("timeline.setProps", props.clone()).unwrap();
            assert!(!playing(&app, &player), "{props}: shown paused at once");
            player.tick(&mut app, &ctx, 5.0);
            assert!(player.playing.is_none(), "{props}");
            assert_eq!(current(&app), 0, "{props}: held");
        }
    }

    #[test]
    fn another_document_stops_playback() {
        let (mut app, ctx, mut player) = app();
        press(&mut app, &ctx, &mut player, TransportAction::Play);
        app.run("file.new", json!({"width": 8, "height": 8})).unwrap();
        app.run("timeline.create", json!({"duration": 10, "fps": 4.0})).unwrap();
        player.tick(&mut app, &ctx, 5.0);
        assert!(!playing(&app, &player) && player.playing.is_none(), "not the one it played");
        assert_eq!(current(&app), 0);
        app.run("file.close", json!({})).unwrap();
        player.tick(&mut app, &ctx, 6.0);
        assert!(!playing(&app, &player), "back to the first: still paused");
    }

    /// Under a dialog (here the unsaved-changes prompt) the bar is refused, as menu clicks are, and
    /// playback stops rather than move the playhead behind it.
    #[test]
    fn the_bar_is_refused_and_playback_stops_under_a_dialog() {
        let (mut app, ctx, mut player) = app();
        app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
        press_at(&mut app, &ctx, &mut player, TransportAction::Play, 0.0).unwrap();
        photocraft_ui_egui::menus::invoke(&mut app, &ctx, "file.close", json!({})).unwrap();
        assert_eq!(app.session.documents().len(), 1, "the prompt asks");
        player.tick(&mut app, &ctx, 1.0);
        assert_eq!((current(&app), player.playing.is_some()), (0, false));
        assert!(press_at(&mut app, &ctx, &mut player, TransportAction::Play, 2.0).is_err());
        assert!(press_at(&mut app, &ctx, &mut player, TransportAction::StepForward, 2.0).is_err());
        assert!(apply(&mut app, &mut player, &ctx, &edit(Phase::Commit, 1, EditValue::Seek { frame: 5 }), 2.0).is_err());
        assert_eq!((current(&app), player.playing.is_some()), (0, false));
    }

    #[test]
    fn other_edits_are_refused() {
        let (mut app, ctx, mut player) = app();
        assert!(apply(&mut app, &mut player, &ctx, &edit(Phase::Commit, 1, EditValue::Number(3.0)), 0.0).is_err());
        let mut elsewhere = edit(Phase::Commit, 1, EditValue::Seek { frame: 3 });
        elsewhere.control = "volume".into();
        assert!(apply(&mut app, &mut player, &ctx, &elsewhere, 0.0).is_err());
        assert_eq!(current(&app), 0);
    }
}
