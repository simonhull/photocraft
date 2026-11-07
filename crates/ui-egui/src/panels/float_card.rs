//! The card-styled floating window: a titled card over the canvas with a close button, used by the
//! floating preset panels (Styles, Shapes, …), and by the dock's panels and the options bar when
//! the host app draws the chrome (`Services::host_chrome`).

use egui::{CornerRadius, Stroke};

use crate::PhotocraftApp;
use crate::theme::Tokens;

/// How big a card's header is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Header {
    /// The desktop's: a small title and a 20-point close box.
    #[default]
    Compact,
    /// For a finger: a larger title, and a close button the size of a touch target (44 points).
    Touch,
}

impl Header {
    /// The title's text size and the close button's side, in points.
    fn metrics(self) -> (f32, f32) {
        match self {
            Header::Compact => (12.0, 20.0),
            Header::Touch => (15.0, 44.0),
        }
    }
}

/// How a card is sized.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CardSize {
    /// A fixed width; the height follows the content.
    Width(f32),
    /// Resizable by its edges, starting at this size.
    Resizable(egui::Vec2),
}

/// A floating card: where it first appears, how big it is and how big its header is. egui
/// remembers where the user moves it, by `id`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FloatCard<'a> {
    pub(crate) id: egui::Id,
    /// Its title, in English: shown translated.
    pub(crate) title: &'a str,
    pub(crate) pos: egui::Pos2,
    pub(crate) size: CardSize,
    pub(crate) header: Header,
}

impl FloatCard<'_> {
    /// Draws the card: a header with the title, then `extras` (a panel menu button, drawn right to
    /// left) and the close button at the end, and `body` below a hairline. True when it was closed.
    pub(crate) fn show(
        self,
        app: &mut PhotocraftApp,
        ctx: &egui::Context,
        extras: impl FnOnce(&mut PhotocraftApp, &mut egui::Ui),
        body: impl FnOnce(&mut PhotocraftApp, &mut egui::Ui),
    ) -> bool {
        let t = Tokens::get(ctx);
        let frame = egui::Frame::NONE
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.card_border))
            .corner_radius(CornerRadius::same(t.radius_lg as u8))
            .shadow(egui::Shadow { offset: [0, 10], blur: 30, spread: 0, color: t.shadow })
            .inner_margin(egui::Margin::same(8));
        let (title_size, close_size) = self.header.metrics();
        let mut close = false;
        let window = egui::Window::new(self.title).id(self.id).title_bar(false).frame(frame).default_pos(self.pos);
        let window = match self.size {
            CardSize::Width(_) => window.resizable(false),
            CardSize::Resizable(size) => window.resizable(true).default_size(size),
        };
        window.show(ctx, |ui| {
            if let CardSize::Width(width) = self.size {
                ui.set_width(width);
            }
            // Photoshop-style tab header with a close box.
            // As tall as the close button, the title centred on it.
            let row = egui::vec2(ui.available_width(), close_size);
            ui.allocate_ui_with_layout(row, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(tl!(self.title)).color(t.text).size(title_size).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::icons::button(ui, "x", close_size, false, tl!("Close")).clicked() {
                        close = true;
                    }
                    extras(app, ui);
                });
            });
            crate::widgets::hairline(ui);
            ui.add_space(4.0);
            body(app, ui);
        });
        close
    }
}
