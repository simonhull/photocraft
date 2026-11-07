//! PhotoCraft's status bar for the shell's native status (craft-mobile's [`Status`]): the message
//! (`ui.status`, red when `status_error`), and the job in view with its progress and Cancel.

use craft_mobile::ui::{Progress, Status};
use photocraft_ui_egui::{PhotocraftApp, jobs_ui};

/// The command a job's Cancel sends: it cancels the job in view, as the status bar's × does.
pub(crate) const CANCEL: &str = "jobs.cancel";

/// The status for the shell, taking PhotoCraft's message (`ui.status`) for it: the shell shows it
/// as a toast, so the status bar it was for no longer needs it, and a message set again, even in
/// the same words, is a new one. Each gets the next number from `said`.
pub(crate) fn take(app: &mut PhotocraftApp, said: &mut u64) -> Status {
    let text = std::mem::take(&mut app.ui.status);
    let error = std::mem::take(&mut app.ui.status_error);
    let mut status = Status::new();
    if !text.trim().is_empty() {
        *said += 1;
        status = if error { status.error(text) } else { status.info(text) }.message_id(*said);
    }
    match jobs_ui::job_in_view(app).filter(|j| j.state == "running") {
        // Nothing reported yet: unknown, as the status bar's bar is indeterminate then.
        Some(job) => {
            let progress = Progress::new(job.label).cancel(CANCEL);
            status.progress(if job.progress > 0.0 { progress.fraction(f64::from(job.progress)) } else { progress })
        }
        None => status,
    }
}

/// Cancel the job in view: the status's Cancel. `false` when there's none, or it can no longer
/// stop (a save writing the file says so in the status).
pub(crate) fn cancel(app: &mut PhotocraftApp) -> bool {
    jobs_ui::job_in_view(app).is_some_and(|job| jobs_ui::cancel(app, job.id))
}

/// The last value sent to the shell, so a value goes out only when it changes.
#[derive(Debug)]
pub(crate) struct Sent<T>(Option<T>);

impl<T> Default for Sent<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T: PartialEq> Sent<T> {
    /// `next`, when it differs from the last value sent: it's the last one sent from now on.
    pub(crate) fn changed(&mut self, next: T) -> Option<&T> {
        if self.0.as_ref() == Some(&next) {
            return None;
        }
        self.0 = Some(next);
        self.0.as_ref()
    }
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

    /// Each message the app sets is a new one, numbered, even in the same words; once taken it's
    /// gone, so the next status has none.
    #[test]
    fn messages_are_taken_and_numbered() {
        let (mut app, mut said) = (app(), 0);
        assert_eq!(take(&mut app, &mut said), Status::new());
        app.ui.status = "Saved a.psd".into();
        assert_eq!(take(&mut app, &mut said), Status::new().info("Saved a.psd").message_id(1));
        assert!(app.ui.status.is_empty());
        assert_eq!(take(&mut app, &mut said), Status::new(), "said once");
        app.ui.status = "Saved a.psd".into();
        assert_eq!(take(&mut app, &mut said), Status::new().info("Saved a.psd").message_id(2), "said again");
        app.ui.status = "Couldn't save".into();
        app.ui.status_error = true;
        assert_eq!(json(&take(&mut app, &mut said))["message"]["tone"], "error");
        assert!(!app.ui.status_error);
    }

    /// A long job shows its progress, and Cancel stops it.
    #[test]
    fn a_running_job_shows_progress_and_cancels() {
        let mut app = app();
        app.background_jobs = true;
        app.run("file.new", json!({"width": 2048, "height": 2048})).unwrap();
        let started = jobs_ui::run(&mut app, "filter.blur.gaussianBlur", json!({"radius": 250})).unwrap();
        assert_eq!(started["pending"], true, "{started}");
        let progress = json(&take(&mut app, &mut 0))["progress"].clone();
        assert_eq!(progress["cancel"], CANCEL, "{progress}");
        assert!(progress["label"].as_str().is_some_and(|l| !l.is_empty()), "{progress}");
        assert!(cancel(&mut app), "Cancel stops it");
    }

    #[test]
    fn a_value_is_sent_only_when_it_changes() {
        let mut sent = Sent::default();
        assert_eq!(sent.changed(Status::new().info("a")), Some(&Status::new().info("a")));
        assert_eq!(sent.changed(Status::new().info("a")), None);
        assert_eq!(sent.changed(Status::new().error("a")), Some(&Status::new().error("a")));
        assert_eq!(sent.changed(Status::new()), Some(&Status::new()));
        assert_eq!(sent.changed(Status::new()), None);
    }
}
