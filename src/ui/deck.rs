//! Bottom bar: panic button, what's playing, and the two output buses.

use super::theme::{self, SURFACE, SURFACE_HI, TEXT, TEXT_DIM, TEXT_FAINT, alpha};
use super::widgets::{self, accent, fmt_time};
use crate::app::RiffApp;
use crate::audio::Cmd;
use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular as ic;

pub fn show(app: &mut RiffApp, ui: &mut Ui) {
    let full = ui.available_rect_before_wrap();
    let h = full.height();
    let right_w = 118.0 + 2.0 * (150.0 + 16.0);
    let stop_rect = Rect::from_min_size(full.left_top() + vec2(0.0, (h - 50.0) / 2.0), vec2(50.0, 50.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(stop_rect), |ui| stop_button(app, ui));

    let compact = full.width() < 900.0;
    let right = Rect::from_min_max(pos2(full.right() - right_w, full.top()), full.right_bottom());
    let mid_right = if compact { full.right() } else { right.left() - 16.0 };
    let mid = Rect::from_min_max(pos2(stop_rect.right() + 16.0, full.top()), pos2(mid_right, full.bottom()));
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(mid).layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| now_playing(app, ui),
    );
    if compact {
        return;
    }
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(right).layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            mic_toggle(app, ui);
            bus(
                ui,
                ic::BROADCAST,
                "Into mic",
                &mut app.settings.mic_bus_volume,
                app.status.peak_mic_bus,
                &mut app.settings_dirty,
                &mut app.mix_dirty,
            );
            bus(
                ui,
                ic::HEADPHONES,
                "You hear",
                &mut app.settings.monitor_volume,
                app.status.peak_monitor,
                &mut app.settings_dirty,
                &mut app.mix_dirty,
            );
        },
    );
}

fn stop_button(app: &mut RiffApp, ui: &mut Ui) {
    let (rect, resp) = ui.allocate_exact_size(vec2(50.0, 50.0), Sense::click());
    let active = !app.status.voices.is_empty();
    let h = ui.ctx().animate_bool_responsive(resp.id, resp.hovered());
    let base = if active { theme::DANGER } else { SURFACE_HI };
    let fill = theme::mix(base, egui::Color32::WHITE, h * 0.1);
    if active {
        let t = ui.input(|i| i.time) as f32;
        ui.painter().circle_filled(rect.center(), 25.0 + 2.0 * (t * 4.0).sin().abs(), alpha(theme::DANGER, 40));
    }
    ui.painter().circle_filled(rect.center(), 23.0, fill);
    ui.painter().rect_filled(
        Rect::from_center_size(rect.center(), vec2(14.0, 14.0)),
        3.0,
        if active { egui::Color32::WHITE } else { TEXT_DIM },
    );
    if resp.on_hover_text("Stop everything (Esc)").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.core.stop_all();
    }
}

fn now_playing(app: &mut RiffApp, ui: &mut Ui) {
    let voices = app.status.voices.clone();
    if voices.is_empty() {
        ui.vertical(|ui| {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Nothing playing").font(theme::semibold(14.0)).color(TEXT_DIM));
            let hint = if app.global_hotkeys_active() {
                "Tap a pad or use a hotkey — works from any app"
            } else {
                "Tap a pad, or press Ctrl+K to search"
            };
            ui.label(widgets::faint(hint));
        });
        return;
    }
    egui::ScrollArea::horizontal().id_salt("np").auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal(|ui| {
            for v in voices.iter().take(6) {
                let (name, emoji, color) = match app.lib.sound(&v.sound_id) {
                    Some(s) => (s.name.clone(), s.emoji.clone(), theme::color(s.color)),
                    None if v.sound_id.starts_with("tts") => ("Speech".into(), "🤖".into(), accent()),
                    None => ("Preview".into(), "🎧".into(), theme::color(2)),
                };
                let (rect, resp) = ui.allocate_exact_size(vec2(200.0, 48.0), Sense::click());
                let p = ui.painter();
                p.rect(rect, 14.0, SURFACE, Stroke::new(1.0, alpha(color, 90)), egui::StrokeKind::Inside);
                let fillw = rect.width() * v.progress;
                p.with_clip_rect(Rect::from_min_size(rect.min, vec2(fillw, rect.height()))).rect_filled(
                    rect.shrink(1.0),
                    13.0,
                    alpha(color, 34),
                );
                let c = pos2(rect.left() + 24.0, rect.center().y);
                // progress ring
                p.circle_stroke(c, 14.0, Stroke::new(2.5, alpha(color, 50)));
                let n = 32;
                let pts: Vec<_> = (0..=((n as f32 * v.progress) as usize))
                    .map(|i| {
                        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / n as f32;
                        c + vec2(a.cos(), a.sin()) * 14.0
                    })
                    .collect();
                if pts.len() > 1 {
                    p.add(egui::Shape::line(pts, Stroke::new(2.5, color)));
                }
                p.text(
                    c,
                    Align2::CENTER_CENTER,
                    &emoji,
                    theme::body(13.0),
                    theme::mix(color, egui::Color32::WHITE, 0.4),
                );
                let tx = rect.left() + 46.0;
                let mut job =
                    egui::text::LayoutJob::single_section(name, egui::TextFormat::simple(theme::semibold(13.0), TEXT));
                job.wrap = egui::text::TextWrapping {
                    max_width: rect.width() - 80.0,
                    max_rows: 1,
                    break_anywhere: true,
                    overflow_character: Some('…'),
                };
                let g = ui.fonts_mut(|f| f.layout_job(job));
                let p = ui.painter();
                p.galley(pos2(tx, rect.top() + 8.0), g, TEXT);
                let status = if v.paused {
                    "Paused".to_string()
                } else if v.looping {
                    format!("{} · loop", fmt_time(v.position))
                } else {
                    format!("-{}", fmt_time(v.duration - v.position))
                };
                p.text(pos2(tx, rect.bottom() - 10.0), Align2::LEFT_BOTTOM, status, theme::body(11.5), TEXT_FAINT);
                let x = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(24.0, 24.0));
                let xr = ui.interact(x, resp.id.with("x"), Sense::click());
                ui.painter().text(
                    x.center(),
                    Align2::CENTER_CENTER,
                    ic::X,
                    theme::body(14.0),
                    if xr.hovered() { TEXT } else { TEXT_FAINT },
                );
                if xr.clicked() {
                    app.core.engine.send(Cmd::StopVoice(v.voice_id));
                } else if resp.clicked() {
                    app.core.engine.send(Cmd::Pause(Some(v.sound_id.clone()), None));
                }
                resp.on_hover_text("Click to pause/resume");
            }
            if voices.len() > 6 {
                ui.label(widgets::dim(format!("+{}", voices.len() - 6)));
            }
        });
    });
}

fn bus(ui: &mut Ui, icon: &str, label: &str, v: &mut f32, level: f32, dirty: &mut bool, mix: &mut bool) {
    const W: f32 = 150.0;
    ui.allocate_ui_with_layout(vec2(W, 50.0), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        let (r, _) = ui.allocate_exact_size(vec2(W, 16.0), Sense::hover());
        let p = ui.painter();
        p.text(r.left_center(), Align2::LEFT_CENTER, format!("{icon}  {label}"), theme::body(12.0), TEXT_DIM);
        p.text(r.right_center(), Align2::RIGHT_CENTER, format!("{:.0}%", *v * 100.0), theme::semibold(12.0), TEXT);
        ui.spacing_mut().slider_width = W;
        ui.spacing_mut().interact_size.y = 16.0;
        if ui.add(egui::Slider::new(v, 0.0..=1.5).show_value(false)).changed() {
            *dirty = true;
            *mix = true;
        }
        widgets::meter(ui, level, W);
    });
}

fn mic_toggle(app: &mut RiffApp, ui: &mut Ui) {
    let on = app.settings.mic_passthrough;
    let live = on && app.status.mic_ok;
    let (rect, resp) = ui.allocate_exact_size(vec2(118.0, 50.0), Sense::click());
    let col = if live {
        theme::OK
    } else if on {
        theme::WARN
    } else {
        TEXT_FAINT
    };
    let h = ui.ctx().animate_bool_responsive(resp.id, resp.hovered());
    ui.painter().rect(
        rect,
        14.0,
        theme::mix(SURFACE, SURFACE_HI, h),
        Stroke::new(1.0, alpha(col, 70)),
        egui::StrokeKind::Inside,
    );
    let c = pos2(rect.left() + 24.0, rect.center().y);
    let lvl = if live { app.status.peak_mic_in.clamp(0.0, 1.0).sqrt() } else { 0.0 };
    ui.painter().circle_filled(c, 13.0 + 5.0 * lvl, alpha(col, (30.0 + 80.0 * lvl) as u8));
    ui.painter().text(
        c,
        Align2::CENTER_CENTER,
        if on { ic::MICROPHONE } else { ic::MICROPHONE_SLASH },
        theme::body(17.0),
        col,
    );
    ui.painter().text(
        pos2(rect.left() + 46.0, rect.top() + 16.0),
        Align2::LEFT_CENTER,
        "Your mic",
        theme::semibold(12.5),
        TEXT,
    );
    ui.painter().text(
        pos2(rect.left() + 46.0, rect.bottom() - 15.0),
        Align2::LEFT_CENTER,
        if live {
            "Mixed in"
        } else if on {
            "Not found"
        } else {
            "Muted"
        },
        theme::body(11.5),
        col,
    );
    if resp
        .on_hover_text("Mix your real microphone into Riff Microphone, so one input carries your voice and your sounds")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        let v = app.core.toggle_mic(None);
        app.settings.mic_passthrough = v;
    }
}
