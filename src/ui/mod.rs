pub mod board;
pub mod deck;
pub mod discover;
pub mod inspector;
pub mod palette;
pub mod settings;
pub mod sidebar;
pub mod speak;
pub mod theme;
pub mod toasts;
pub mod widgets;

use crate::app::{RiffApp, View};
use egui::{Margin, Stroke};

pub fn draw(app: &mut RiffApp, ui: &mut egui::Ui) {
    egui::Panel::left("sidebar")
        .exact_size(236.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(theme::SIDEBAR).inner_margin(Margin {
            left: 14,
            right: 14,
            top: 16,
            bottom: 14,
        }))
        .show(ui, |ui| sidebar::show(app, ui));

    egui::Panel::bottom("deck")
        .exact_size(78.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(theme::SIDEBAR).stroke(Stroke::new(1.0, theme::BORDER)).inner_margin(Margin {
            left: 18,
            right: 18,
            top: 12,
            bottom: 12,
        }))
        .show(ui, |ui| deck::show(app, ui));

    let show_inspector = app.view == View::Sounds
        && app.settings.show_inspector
        && app.selected.as_ref().is_some_and(|id| app.visible_sounds().iter().any(|s| &s.id == id));
    if show_inspector {
        egui::Panel::right("inspector")
            .exact_size(330.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE_LO)
                    .stroke(Stroke::new(1.0, theme::BORDER))
                    .inner_margin(Margin::same(18)),
            )
            .show(ui, |ui| inspector::show(app, ui));
    }

    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(theme::BG).inner_margin(Margin { left: 28, right: 28, top: 22, bottom: 8 }))
        .show(ui, |ui| match app.view {
            View::Sounds => board::show(app, ui),
            View::Speak => speak::show(app, ui),
            View::Discover => discover::show(app, ui),
            View::Settings => settings::show(app, ui),
        });

    let ctx = ui.ctx().clone();
    drop_overlay(app, &ctx);
    palette::show(app, &ctx);
    app.toasts.show(&ctx);
}

fn drop_overlay(app: &RiffApp, ctx: &egui::Context) {
    let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
    if !hovering {
        return;
    }
    let board = app.lib.board(&app.current_board_id()).map(|b| b.name.clone()).unwrap_or_default();
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop")));
    painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(170));
    let r = screen.shrink(40.0);
    painter.rect_stroke(r, 24.0, Stroke::new(2.0, widgets::accent()), egui::StrokeKind::Inside);
    painter.text(
        r.center() - egui::vec2(0.0, 26.0),
        egui::Align2::CENTER_CENTER,
        egui_phosphor::regular::DOWNLOAD_SIMPLE,
        theme::body(48.0),
        widgets::accent(),
    );
    painter.text(
        r.center() + egui::vec2(0.0, 24.0),
        egui::Align2::CENTER_CENTER,
        format!("Drop to add to {board}"),
        theme::display(24.0),
        theme::TEXT,
    );
}
