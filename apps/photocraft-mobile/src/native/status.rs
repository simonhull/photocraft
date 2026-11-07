//! PhotoCraft's status bar for the shell's native status (craft-mobile's [`Status`]): the message
//! (`ui.status`, red when `status_error`), and the job in view with its progress and Cancel.

use craft_mobile::ui::{Progress, Status};
use photocraft_ui_egui::{PhotocraftApp, jobs_ui};

/// The command a job's Cancel sends: it cancels the job in view, as the status bar's × does.
pub(crate) const CANCEL: &str = "jobs.cancel";

/// What PhotoCraft has said to the shell: the message standing, numbered, so the shell can tell a
/// new message from the same one updated.
///
/// PhotoCraft's status bar has no notion of a new message: a command sets `ui.status` once, while
/// a lasting condition (a low scratch disk, "retrying in N s") sets it again every frame. So a
/// message set in the frame right after one was, in the same tone and the same words but for its
/// numbers, continues it, and keeps its number; anything else is a new message.
#[derive(Debug, Default)]
pub(crate) struct Said {
    /// The message standing: its text, whether it's an error, and its number.
    message: Option<(String, bool, u64)>,
    /// How many messages have been said; the last one's number.
    count: u64,
    /// The last frame set a message, which this frame's may continue.
    continuing: bool,
}

impl Said {
    /// How many messages have been said.
    #[cfg(test)]
    pub(crate) fn count(&self) -> u64 {
        self.count
    }

    /// The message `text` was set this frame (an error when `error`); empty text, none was.
    fn hear(&mut self, text: String, error: bool) {
        if text.trim().is_empty() {
            self.continuing = false;
            return;
        }
        let continues = self.continuing && self.message.as_ref().is_some_and(|(said, was_error, _)| *was_error == error && same_but_numbers(said, &text));
        if !continues {
            self.count += 1;
        }
        self.message = Some((text, error, self.count));
        self.continuing = true;
    }

    /// The status with the message standing, if any.
    fn status(&self) -> Status {
        match &self.message {
            Some((text, true, id)) => Status::new().error(text.as_str()).message_id(*id),
            Some((text, false, id)) => Status::new().info(text.as_str()).message_id(*id),
            None => Status::new(),
        }
    }
}

/// The same words, numbers aside ("retrying in 3 s", "retrying in 10 s").
fn same_but_numbers(a: &str, b: &str) -> bool {
    let words = |s: &str| s.chars().filter(|c| !c.is_ascii_digit()).collect::<String>();
    words(a) == words(b)
}

/// The status for the shell, taking PhotoCraft's message (`ui.status`) for it: the shell shows it
/// as a toast, so the status bar it was for no longer needs it. The message stands until the next
/// one ([`Said`]).
pub(crate) fn take(app: &mut PhotocraftApp, said: &mut Said) -> Status {
    said.hear(std::mem::take(&mut app.ui.status), std::mem::take(&mut app.ui.status_error));
    let status = said.status();
    match jobs_ui::job_in_view(app).filter(|j| j.state == "running") {
        // Nothing reported yet: unknown, as the status bar's bar is indeterminate then.
        Some(job) => {
            let progress = Progress::new(job.label).cancel(CANCEL);
            status.progress(if job.progress > 0.0 { progress.fraction(f64::from(job.progress)) } else { progress })
        }
        None => status,
    }
}

/// Cancel the job in view: the status's Cancel. `false` when there's none, it can no longer stop
/// (a save writing the file says so in the status), or a dialog or prompt holds the app.
pub(crate) fn cancel(app: &mut PhotocraftApp) -> bool {
    // Refused while a dialog or prompt holds the app, as the menus are.
    if !photocraft_ui_egui::menus::modal_allows(app, CANCEL) {
        return false;
    }
    jobs_ui::job_in_view(app).is_some_and(|job| jobs_ui::cancel(app, job.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_engine::Session;
    use photocraft_ui_egui::Services;
    use serde_json::{Value, json};

    fn app() -> PhotocraftApp {
        PhotocraftApp::new(Session::new(), Services::default())
    }

    fn json(status: &Status) -> Value {
        serde_json::to_value(status).unwrap()
    }

    /// Each message the app sets is a new one, numbered, even in the same words; taken from the
    /// status bar, it stands until the next, so the status doesn't change while nothing new is said.
    #[test]
    fn messages_are_taken_numbered_and_stand() {
        let (mut app, mut said) = (app(), Said::default());
        assert_eq!(take(&mut app, &mut said), Status::new());
        app.ui.status = "Saved a.psd".into();
        assert_eq!(take(&mut app, &mut said), Status::new().info("Saved a.psd").message_id(1));
        assert!(app.ui.status.is_empty());
        assert_eq!(take(&mut app, &mut said), Status::new().info("Saved a.psd").message_id(1), "it stands");
        app.ui.status = "Saved a.psd".into();
        assert_eq!(take(&mut app, &mut said), Status::new().info("Saved a.psd").message_id(2), "said again");
        take(&mut app, &mut said);
        app.ui.status = "Couldn't save".into();
        app.ui.status_error = true;
        assert_eq!(json(&take(&mut app, &mut said))["message"], json!({"text": "Couldn't save", "tone": "error", "id": 3}));
        assert!(!app.ui.status_error);
    }

    /// A low scratch disk says "retrying in N s" every frame, counting down: one message, its text
    /// updated (the shell updates its toast in place), until a frame says nothing.
    #[test]
    fn a_countdown_set_every_frame_is_one_message() {
        let (mut app, mut said) = (app(), Said::default());
        let mut frame = |app: &mut PhotocraftApp, text: &str, error: bool| {
            app.ui.status = text.into();
            app.ui.status_error = error;
            take(app, &mut said).message().map(|m| (m.text().to_owned(), m.id()))
        };
        let low = |n: u32| format!("disk full; spilling is paused (tiles stay in memory), retrying in {n} s");
        assert_eq!(frame(&mut app, &low(3), true), Some((low(3), 1)));
        assert_eq!(frame(&mut app, &low(3), true), Some((low(3), 1)), "said again, the same message");
        assert_eq!(frame(&mut app, &low(2), true), Some((low(2), 1)), "counting: updated");
        assert_eq!(frame(&mut app, &low(10), true), Some((low(10), 1)));
        assert_eq!(frame(&mut app, "Couldn't save a.psd", true), Some(("Couldn't save a.psd".into(), 2)), "other words: new");
        assert_eq!(frame(&mut app, &low(1), true), Some((low(1), 3)), "and back: new again");
        assert_eq!(frame(&mut app, "Saved 2 files", false), Some(("Saved 2 files".into(), 4)), "another tone: new");
        assert_eq!(frame(&mut app, "", false), Some(("Saved 2 files".into(), 4)), "nothing said: it stands");
        assert_eq!(frame(&mut app, "Saved 3 files", false), Some(("Saved 3 files".into(), 5)), "after a pause: new");
    }

    /// A long job shows its progress, and Cancel stops it.
    #[test]
    fn a_running_job_shows_progress_and_cancels() {
        let mut app = app();
        app.background_jobs = true;
        app.run("file.new", json!({"width": 2048, "height": 2048})).unwrap();
        let started = jobs_ui::run(&mut app, "filter.blur.gaussianBlur", json!({"radius": 250})).unwrap();
        assert_eq!(started["pending"], true, "{started}");
        let progress = json(&take(&mut app, &mut Said::default()))["progress"].clone();
        assert_eq!(progress["cancel"], CANCEL, "{progress}");
        assert!(progress["label"].as_str().is_some_and(|l| !l.is_empty()), "{progress}");
        assert!(cancel(&mut app), "Cancel stops it");
    }

    /// Under the unsaved-changes prompt Cancel is refused, as the menus are.
    #[test]
    fn cancel_is_refused_under_the_prompt() {
        let mut app = app();
        app.background_jobs = true;
        app.run("file.new", json!({"width": 2048, "height": 2048})).unwrap();
        app.run("edit.fill", json!({"color": "#ff0000"})).unwrap();
        jobs_ui::run(&mut app, "filter.blur.gaussianBlur", json!({"radius": 250})).unwrap();
        photocraft_ui_egui::menus::invoke(&mut app, &egui::Context::default(), "file.close", json!({})).unwrap();
        assert!(!photocraft_ui_egui::menus::modal_allows(&app, "file.new"), "the prompt is up");
        assert!(!cancel(&mut app));
    }
}
