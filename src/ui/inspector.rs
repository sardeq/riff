use super::theme::{self, BORDER, SURFACE_HI, TEXT, TEXT_DIM, TEXT_FAINT, alpha};
use super::widgets::{self, accent, fmt_time, paint_waveform};
use crate::app::{RecordTarget, RiffApp};
use crate::model::{SoundFx, SoundSource};
use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular as ic;

const EMOJIS: &[&str] = &[
    "🔊", "📯", "🥁", "🎸", "🎹", "🎺", "🎷", "🎻", "🎤", "🎧", "🎵", "🎶", "😂", "🤣", "😭", "😱", "😮", "😐", "😎",
    "🤔", "🙄", "😈", "🤡", "👻", "💀", "🤖", "👽", "🐱", "🐶", "🐸", "🦆", "🐐", "🐄", "🔥", "💥", "⚡", "✨", "💫",
    "🌀", "💨", "💧", "❄", "⛈", "🌈", "🚨", "🔔", "⏰", "📢", "🎉", "🏆", "👏", "👋", "👍", "👎", "🙅", "🤐", "💩",
    "❤", "💔", "⭐", "🍕", "🍔", "🚗", "🚀", "🔫", "🗡", "💣", "🎮", "🕹", "🎲", "🃏", "❓",
];

pub fn show(app: &mut RiffApp, ui: &mut Ui) {
    let Some(id) = app.selected.clone() else { return };
    let Some(orig) = app.lib.sound(&id).cloned() else { return };
    let mut s = orig.clone();
    let color = theme::color(s.color);
    let playing: Vec<_> = app.status.voices.iter().filter(|v| v.sound_id == id).cloned().collect();
    let picker_id = ui.id().with("emoji-picker");
    let mut picker = ui.data(|d| d.get_temp::<bool>(picker_id).unwrap_or(false));

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.set_max_width(ui.available_width());
        // ---------- header ----------
        ui.horizontal(|ui| {
            widgets::label_caps(ui, "Edit sound");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::small_icon_button(ui, ic::X, "Close").clicked() {
                    app.selected = None;
                }
            });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::click());
            widgets::gradient_rounded_rect(ui.painter(), r, 16.0, alpha(color, 90), alpha(color, 30));
            ui.painter().text(
                r.center(),
                Align2::CENTER_CENTER,
                &s.emoji,
                theme::body(28.0),
                theme::mix(color, egui::Color32::WHITE, 0.4),
            );
            if resp.on_hover_text("Change icon").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                picker = !picker;
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(
                    egui::TextEdit::singleline(&mut s.name)
                        .font(theme::semibold(17.0))
                        .frame(egui::Frame::NONE)
                        .desired_width(ui.available_width()),
                );
                ui.label(widgets::faint(source_line(&s.source, s.duration)));
            });
        });
        if picker {
            ui.add_space(6.0);
            egui::Frame::new().fill(theme::SURFACE).corner_radius(12).inner_margin(8).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                    for e in EMOJIS {
                        let (r, resp) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
                        if resp.hovered() || s.emoji == *e {
                            ui.painter().rect_filled(r, 8.0, SURFACE_HI);
                        }
                        ui.painter().text(r.center(), Align2::CENTER_CENTER, *e, theme::body(18.0), TEXT);
                        if resp.clicked() {
                            s.emoji = e.to_string();
                            picker = false;
                        }
                    }
                });
            });
        }

        // ---------- color ----------
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            for (i, (c, name)) in theme::PALETTE.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                ui.painter().circle_filled(r.center(), 9.0, *c);
                if s.color == i {
                    ui.painter().circle_stroke(r.center(), 11.0, Stroke::new(2.0, TEXT));
                }
                if resp.on_hover_text(*name).clicked() {
                    s.color = i;
                }
            }
        });

        // ---------- transport ----------
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let is_playing = !playing.is_empty();
            let (icon, label) = if is_playing { (ic::STOP, "Stop") } else { (ic::PLAY, "Play") };
            if widgets::primary_button(ui, icon, label).clicked() {
                if is_playing { app.core.stop(&id) } else { app.core.play(&id, true) }
            }
            let fav_col = if s.favorite { theme::color(3) } else { TEXT_DIM };
            if widgets::icon_button(ui, ic::STAR, if s.favorite { "Unfavorite" } else { "Favorite" }).clicked() {
                s.favorite = !s.favorite;
            }
            let _ = fav_col;
            if let Some(v) = playing.first() {
                ui.label(
                    egui::RichText::new(format!("{} / {}", fmt_time(v.position), fmt_time(v.duration))).color(color),
                );
            }
        });

        // ---------- waveform + trim ----------
        ui.add_space(12.0);
        trim_editor(app, ui, &id, &mut s.fx, color, playing.first().map(|v| v.progress));

        // ---------- effects ----------
        ui.add_space(14.0);
        widgets::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            widgets::slider_row(ui, "Volume", &mut s.fx.volume, 0.0..=2.0, |v| format!("{:.0}%", v * 100.0));
            widgets::slider_row(ui, "Speed & pitch", &mut s.fx.speed, 0.5..=2.0, |v| format!("{v:.2}×"));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (label, v) in [("Demon", 0.6), ("Slow", 0.8), ("Normal", 1.0), ("Fast", 1.25), ("Chipmunk", 1.6)] {
                    let sel = (s.fx.speed - v).abs() < 0.01;
                    let col = if sel { accent() } else { TEXT_FAINT };
                    if widgets::chip(ui, label, col)
                        .interact(Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        s.fx.speed = v;
                    }
                }
            });
            ui.add_space(4.0);
            widgets::slider_row(ui, "Bass boost", &mut s.fx.bass_db, -12.0..=18.0, |v| format!("{v:+.0} dB"));
            widgets::slider_row(ui, "Echo", &mut s.fx.echo, 0.0..=1.0, |v| {
                if v < 0.01 { "Off".into() } else { format!("{:.0}%", v * 100.0) }
            });
            widgets::slider_row(ui, "Fade in", &mut s.fx.fade_in, 0.0..=3.0, |v| {
                if v < 0.01 { "Off".into() } else { format!("{v:.1}s") }
            });
            widgets::slider_row(ui, "Fade out", &mut s.fx.fade_out, 0.0..=3.0, |v| {
                if v < 0.01 { "Off".into() } else { format!("{v:.1}s") }
            });
            ui.add_space(2.0);
            widgets::toggle_row(ui, "Loop", "Repeat until stopped", &mut s.fx.looping);
            if s.fx != (SoundFx { to_mic: s.fx.to_mic, to_speakers: s.fx.to_speakers, ..Default::default() }) {
                ui.add_space(2.0);
                if ui.link(widgets::faint(format!("{}  Reset effects", ic::ARROWS_CLOCKWISE))).clicked() {
                    s.fx = SoundFx { to_mic: s.fx.to_mic, to_speakers: s.fx.to_speakers, ..Default::default() };
                }
            }
        });

        // ---------- routing ----------
        ui.add_space(12.0);
        widgets::card(ui, |ui| {
            widgets::label_caps(ui, "Send to");
            widgets::toggle_row(
                ui,
                &format!("{}  Riff Microphone", ic::MICROPHONE),
                "What others hear in voice chat",
                &mut s.fx.to_mic,
            );
            widgets::toggle_row(
                ui,
                &format!("{}  My speakers", ic::HEADPHONES),
                "What you hear",
                &mut s.fx.to_speakers,
            );
        });

        // ---------- hotkey ----------
        ui.add_space(12.0);
        widgets::card(ui, |ui| {
            widgets::label_caps(ui, "Hotkey");
            let recording = app.recording == Some(RecordTarget::Sound(id.clone()));
            ui.horizontal(|ui| {
                let label = if recording {
                    "Press a key combo…  (Esc cancels)".to_string()
                } else {
                    s.hotkey.as_ref().map(|h| h.label()).unwrap_or_else(|| "Not set".into())
                };
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 34.0, 34.0), Sense::click());
                let a = if recording { 0.5 + 0.5 * (ui.input(|i| i.time) as f32 * 5.0).sin() } else { 0.0 };
                ui.painter().rect(
                    r,
                    10.0,
                    theme::SURFACE_LO,
                    Stroke::new(1.0, if recording { alpha(accent(), (120.0 + 120.0 * a) as u8) } else { BORDER }),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    r.left_center() + vec2(12.0, 0.0),
                    Align2::LEFT_CENTER,
                    format!("{}  {label}", ic::KEYBOARD),
                    theme::semibold(13.0),
                    if s.hotkey.is_some() || recording { TEXT } else { TEXT_FAINT },
                );
                if recording {
                    ui.ctx().request_repaint();
                }
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    if recording {
                        app.finish_recording(None)
                    } else {
                        app.start_recording(RecordTarget::Sound(id.clone()))
                    }
                }
                if s.hotkey.is_some() && widgets::small_icon_button(ui, ic::X, "Clear hotkey").clicked() {
                    s.hotkey = None;
                }
            });
            let global = app.global_hotkeys_active();
            ui.label(widgets::faint(if global {
                "Works everywhere, even when Riff is in the background."
            } else {
                "Only while Riff is focused. Enable global hotkeys in Settings, or bind it in your compositor:"
            }));
            if let Some(hk) = &s.hotkey {
                ui.horizontal_wrapped(|ui| {
                    let name = s.name.clone();
                    if ui.small_button("Copy niri bind").clicked() {
                        ui.ctx().copy_text(hk.niri_bind(&["play", &name]));
                        app.toast(
                            crate::core::ToastKind::Success,
                            "niri bind copied — paste it into binds { } in config.kdl",
                        );
                    }
                    if ui.small_button("Copy Hyprland bind").clicked() {
                        ui.ctx().copy_text(hk.hyprland_bind(&["play", &name]));
                        app.toast(crate::core::ToastKind::Success, "Hyprland bind copied");
                    }
                });
            }
        });

        // ---------- footer ----------
        ui.add_space(12.0);
        ui.label(widgets::faint(format!(
            "Played {} time{}{}",
            s.plays,
            if s.plays == 1 { "" } else { "s" },
            if s.path.as_os_str().is_empty() { String::new() } else { format!(" · {}", s.path.display()) }
        )));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if widgets::ghost_button(ui, ic::COPY, "Duplicate").clicked() {
                app.duplicate_sound(&id);
            }
            if widgets::danger_button(ui, ic::TRASH, "Delete").clicked() {
                app.delete_sound(&id);
            }
        });
        ui.add_space(12.0);
    });

    ui.data_mut(|d| d.insert_temp(picker_id, picker));
    if app.lib.sound(&id).is_some() {
        s.fx.trim_end = s.fx.trim_end.max(s.fx.trim_start + 0.01).min(1.0);
        let changed = s.name != orig.name
            || s.emoji != orig.emoji
            || s.color != orig.color
            || s.fx != orig.fx
            || s.hotkey != orig.hotkey
            || s.favorite != orig.favorite;
        if changed {
            let new = s.clone();
            app.edit_sound(&id, move |x| {
                x.name = if new.name.trim().is_empty() { x.name.clone() } else { new.name };
                x.emoji = new.emoji;
                x.color = new.color;
                x.fx = new.fx;
                x.hotkey = new.hotkey;
                x.favorite = new.favorite;
            });
        }
    }
}

fn source_line(src: &SoundSource, dur: f32) -> String {
    let d = if dur > 0.0 { format!(" · {}", fmt_time(dur)) } else { String::new() };
    match src {
        SoundSource::Local => format!("Imported file{d}"),
        SoundSource::Builtin => format!("Riff starter sound{d}"),
        SoundSource::Tts { voice, .. } => format!("Text to speech · {voice}{d}"),
        SoundSource::Freesound { username, license, .. } => format!("Freesound · by {username} · {license}{d}"),
    }
}

fn trim_editor(app: &RiffApp, ui: &mut Ui, id: &str, fx: &mut SoundFx, color: egui::Color32, progress: Option<f32>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 76.0), Sense::hover());
    let p = ui.painter();
    p.rect(rect, 12.0, theme::SURFACE, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
    let wave = rect.shrink2(vec2(10.0, 12.0));
    let Some(clip) = app.core.clip(id) else {
        p.text(rect.center(), Align2::CENTER_CENTER, "Decoding…", theme::body(12.0), TEXT_FAINT);
        return;
    };
    let x_of = |t: f32| wave.left() + wave.width() * t;
    // playhead progress is relative to the trimmed region
    let abs_prog = progress.map(|p| fx.trim_start + p * (fx.trim_end - fx.trim_start));
    paint_waveform(
        p,
        wave,
        &clip.peaks,
        abs_prog.or(Some(0.0)),
        theme::mix(color, egui::Color32::WHITE, 0.15),
        alpha(color, 110),
    );
    // dim the trimmed-away parts
    let dim = egui::Color32::from_black_alpha(150);
    p.rect_filled(Rect::from_min_max(rect.min, pos2(x_of(fx.trim_start), rect.bottom())).intersect(rect), 12.0, dim);
    p.rect_filled(Rect::from_min_max(pos2(x_of(fx.trim_end), rect.top()), rect.max).intersect(rect), 12.0, dim);

    let handle = |ui: &mut Ui, t: &mut f32, lo: f32, hi: f32, key: &str| {
        let x = wave.left() + wave.width() * *t;
        let hr = Rect::from_center_size(pos2(x, rect.center().y), vec2(14.0, rect.height()));
        let resp = ui
            .interact(hr, ui.id().with(("trim", key)), Sense::drag())
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        if resp.dragged()
            && let Some(pos) = resp.interact_pointer_pos()
        {
            *t = ((pos.x - wave.left()) / wave.width()).clamp(lo, hi);
        }
        let active = resp.hovered() || resp.dragged();
        let c = if active { TEXT } else { accent() };
        ui.painter().line_segment([pos2(x, rect.top() + 4.0), pos2(x, rect.bottom() - 4.0)], Stroke::new(2.0, c));
        ui.painter().rect_filled(Rect::from_center_size(pos2(x, rect.center().y), vec2(6.0, 18.0)), 3.0, c);
        if resp.dragged() {
            ui.painter().text(
                pos2(x, rect.top() - 2.0),
                Align2::CENTER_BOTTOM,
                fmt_time(*t * clip.duration()),
                theme::body(11.0),
                TEXT,
            );
        }
    };
    let (ts, te) = (fx.trim_start, fx.trim_end);
    handle(ui, &mut fx.trim_start, 0.0, (te - 0.01).max(0.0), "start");
    handle(ui, &mut fx.trim_end, (ts + 0.01).min(1.0), 1.0, "end");

    ui.horizontal(|ui| {
        ui.label(widgets::faint(format!(
            "Trim {} → {}  ({})",
            fmt_time(fx.trim_start * clip.duration()),
            fmt_time(fx.trim_end * clip.duration()),
            fmt_time((fx.trim_end - fx.trim_start) * clip.duration())
        )));
        if (fx.trim_start > 0.0 || fx.trim_end < 1.0) && ui.link(widgets::faint("Reset")).clicked() {
            fx.trim_start = 0.0;
            fx.trim_end = 1.0;
        }
    });
}
