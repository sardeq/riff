use super::sidebar::DragSound;
use super::theme::{self, BORDER, SURFACE, SURFACE_HI, TEXT, TEXT_DIM, TEXT_FAINT, alpha};
use super::widgets::{self, accent, fmt_time, paint_waveform};
use crate::app::{BoardSel, RecordTarget, RiffApp, View};
use crate::model::Sound;
use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular as ic;

const GAP: f32 = 14.0;

pub fn show(app: &mut RiffApp, ui: &mut Ui) {
    header(app, ui);
    ui.add_space(14.0);
    let sounds = app.visible_sounds();
    if sounds.is_empty() {
        empty_state(app, ui);
        return;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        grid(app, ui, &sounds);
        ui.add_space(20.0);
    });
}

fn header(app: &mut RiffApp, ui: &mut Ui) {
    let (title, color) = match &app.board {
        BoardSel::All => ("All sounds".to_string(), accent()),
        BoardSel::Favorites => ("Favorites".to_string(), theme::color(3)),
        BoardSel::Recent => ("Recently played".to_string(), theme::color(2)),
        BoardSel::Board(id) => {
            app.lib.board(id).map(|b| (b.name.clone(), theme::color(b.color))).unwrap_or(("Board".into(), accent()))
        }
    };
    let sounds = app.visible_sounds();
    let with_keys = sounds.iter().filter(|s| s.hotkey.is_some()).count();
    let total: f32 = sounds.iter().map(|s| s.duration).sum();

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 28.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 5.0, color);
                ui.label(egui::RichText::new(&title).font(theme::display(28.0)).color(TEXT));
            });
            let mut sub = format!("{} sound{}", sounds.len(), if sounds.len() == 1 { "" } else { "s" });
            if total > 0.0 {
                sub.push_str(&format!(" · {}", fmt_time(total)));
            }
            if with_keys > 0 {
                sub.push_str(&format!(" · {with_keys} on hotkeys"));
            }
            ui.label(widgets::dim(sub));
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::primary_button(ui, ic::PLUS, "Add sounds")
                .on_hover_text("Ctrl+O · or drop files anywhere")
                .clicked()
            {
                app.import_dialog();
            }
            let board = match &app.board {
                BoardSel::Board(id) => Some(id.clone()),
                _ => None,
            };
            if widgets::icon_button(ui, ic::SHUFFLE, "Play a random sound").clicked() {
                app.core.random(board.as_deref());
            }
            let narrow = ui.available_width() < 330.0;
            if !narrow && widgets::icon_button(ui, ic::COMMAND, "Command palette (Ctrl+K)").clicked() {
                app.palette.toggle();
            }
            let id = egui::Id::new("board-search");
            let w = if narrow { 110.0 } else { 170.0 };
            let r = widgets::search_field(
                ui,
                &mut app.search,
                if narrow { "Search…" } else { "Search sounds…  Ctrl+F" },
                w,
                id,
            );
            if app.focus_search {
                r.request_focus();
                app.focus_search = false;
            }
            if r.has_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && let Some(first) = app.visible_sounds().first()
            {
                app.core.trigger(&first.id);
            }
        });
    });
}

fn grid(app: &mut RiffApp, ui: &mut Ui, sounds: &[Sound]) {
    let scale = app.settings.pad_size.clamp(0.75, 1.5);
    let min_w = 168.0 * scale;
    let h = 132.0 * scale;
    let avail = ui.available_width();
    let cols = ((avail + GAP) / (min_w + GAP)).floor().max(1.0) as usize;
    let w = (avail - GAP * (cols - 1) as f32) / cols as f32;
    let rows = sounds.len().div_ceil(cols);
    let total_h = rows as f32 * (h + GAP);
    let (area, _) = ui.allocate_exact_size(vec2(avail, total_h), Sense::hover());
    let clip = ui.clip_rect();
    let time = ui.input(|i| i.time) as f32;

    let mut reorder: Option<(String, String)> = None;
    for (i, s) in sounds.iter().enumerate() {
        let (r, c) = (i / cols, i % cols);
        let rect = Rect::from_min_size(area.min + vec2(c as f32 * (w + GAP), r as f32 * (h + GAP)), vec2(w, h));
        if !clip.intersects(rect.expand(20.0)) {
            continue;
        }
        if let Some(target) = pad(app, ui, s, rect, time) {
            reorder = Some((target, s.id.clone()));
        }
    }
    if let Some((moved, before)) = reorder
        && moved != before
    {
        app.edit_lib(|lib| {
            if let Some(from) = lib.sounds.iter().position(|s| s.id == moved) {
                let item = lib.sounds.remove(from);
                let to = lib.sounds.iter().position(|s| s.id == before).unwrap_or(lib.sounds.len());
                lib.sounds.insert(to, item);
            }
        });
    }
}

/// Draws one pad. Returns the id of a pad dropped onto this one (for reordering).
fn pad(app: &mut RiffApp, ui: &mut Ui, s: &Sound, rect: Rect, time: f32) -> Option<String> {
    let id = ui.id().with(("pad", &s.id));
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let color = theme::color(s.color);
    let voices: Vec<_> = app.status.voices.iter().filter(|v| v.sound_id == s.id).collect();
    let playing = !voices.is_empty();
    let progress = voices.iter().map(|v| v.progress).fold(None, |a: Option<f32>, p| Some(a.map_or(p, |a| a.max(p))));
    let selected = app.selected.as_deref() == Some(&s.id);
    let clip = app.core.clip(&s.id);
    let failed = app.core.failed.read().get(&s.id).cloned();
    let recording = app.recording == Some(RecordTarget::Sound(s.id.clone()));

    let hover = ui.ctx().animate_bool_responsive(id.with("hover"), resp.hovered());
    let glow = ui.ctx().animate_bool_with_time(id.with("glow"), playing, 0.25);
    let press = resp.is_pointer_button_down_on() && !resp.dragged();
    let rect_draw = if press { rect.shrink(2.0) } else { rect.translate(vec2(0.0, -2.0 * hover)) };
    let p = ui.painter();

    // shadow / glow
    if glow > 0.0 {
        let pulse = 0.75 + 0.25 * (time * 4.0).sin();
        let sh =
            egui::Shadow { offset: [0, 0], blur: 28, spread: 1, color: alpha(color, (110.0 * glow * pulse) as u8) };
        p.add(sh.as_shape(rect_draw, 18.0));
    } else if hover > 0.0 {
        let sh = egui::Shadow {
            offset: [0, 8],
            blur: 20,
            spread: 0,
            color: egui::Color32::from_black_alpha((90.0 * hover) as u8),
        };
        p.add(sh.as_shape(rect_draw, 18.0));
    }

    let fill = theme::mix(SURFACE, SURFACE_HI, hover * 0.7);
    let fill = theme::mix(fill, color, 0.07 + glow * 0.06);
    let stroke = if failed.is_some() {
        Stroke::new(1.5, alpha(theme::DANGER, 160))
    } else if selected {
        Stroke::new(2.0, accent())
    } else if glow > 0.0 {
        Stroke::new(1.5, alpha(color, (200.0 * glow) as u8))
    } else {
        Stroke::new(1.0, theme::mix(BORDER, color, hover * 0.5))
    };
    p.rect(rect_draw, 18.0, fill, stroke, egui::StrokeKind::Inside);

    // progress wash
    if let Some(pr) = progress {
        let wash =
            Rect::from_min_max(rect_draw.min, pos2(rect_draw.left() + rect_draw.width() * pr, rect_draw.bottom()));
        p.with_clip_rect(wash).rect_filled(rect_draw.shrink(1.0), 17.0, alpha(color, 26));
    }

    let pad_in = 14.0;
    let inner = rect_draw.shrink(pad_in);

    // icon bubble
    let bubble_r = 20.0 + if playing { 1.5 * (time * 6.0).sin() } else { 0.0 };
    let bc = pos2(inner.left() + 20.0, inner.top() + 20.0);
    p.circle_filled(bc, bubble_r, alpha(color, 44));
    if playing {
        p.circle_stroke(
            bc,
            bubble_r + 4.0 + 3.0 * ((time * 3.0).sin() * 0.5 + 0.5),
            Stroke::new(1.0, alpha(color, 70)),
        );
    }
    p.text(bc, Align2::CENTER_CENTER, &s.emoji, theme::body(20.0), theme::mix(color, egui::Color32::WHITE, 0.35));

    // top-right: hotkey badge / favorite / loop
    let mut x = inner.right();
    if recording {
        let a = (0.5 + 0.5 * (time * 6.0).sin()) * 255.0;
        x = badge(p, x, inner.top() + 6.0, "press keys…", alpha(accent(), a as u8), TEXT);
        ui.ctx().request_repaint();
    } else if let Some(hk) = &s.hotkey {
        x = badge(p, x, inner.top() + 6.0, &hk.short(), theme::SURFACE_LO, TEXT_DIM);
    }
    if s.favorite {
        p.text(pos2(x - 4.0, inner.top() + 14.0), Align2::RIGHT_CENTER, ic::STAR, theme::body(13.0), theme::color(3));
        x -= 20.0;
    }
    if s.fx.looping {
        p.text(pos2(x - 4.0, inner.top() + 14.0), Align2::RIGHT_CENTER, ic::REPEAT, theme::body(13.0), TEXT_FAINT);
    }

    // name (max 2 lines)
    let wave_h = 22.0 * app.settings.pad_size.clamp(0.75, 1.5);
    let name_top = inner.top() + 48.0;
    let mut job =
        egui::text::LayoutJob::single_section(s.name.clone(), egui::TextFormat::simple(theme::semibold(14.0), TEXT));
    job.wrap = egui::text::TextWrapping {
        max_width: inner.width(),
        max_rows: 2,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let p = ui.painter();
    p.galley(pos2(inner.left(), name_top), galley, TEXT);

    // waveform + time
    let wave =
        Rect::from_min_max(pos2(inner.left(), inner.bottom() - wave_h), pos2(inner.right() - 44.0, inner.bottom()));
    match (&clip, &failed) {
        (Some(c), _) => match progress {
            Some(pr) => {
                let on = theme::mix(color, egui::Color32::WHITE, 0.15);
                paint_waveform(p, wave, &c.peaks, Some(pr), on, alpha(color, 60));
            }
            None => {
                let idle = theme::mix(TEXT_FAINT, color, 0.3 + 0.45 * hover);
                paint_waveform(p, wave, &c.peaks, None, idle, idle);
            }
        },
        (None, Some(_)) => {
            p.text(
                wave.left_center(),
                Align2::LEFT_CENTER,
                format!("{}  can't decode", ic::WARNING),
                theme::body(12.0),
                theme::DANGER,
            );
        }
        (None, None) => {
            // shimmer while decoding
            let t = (time * 1.5).fract();
            let shimmer =
                Rect::from_center_size(pos2(wave.left() + wave.width() * t, wave.center().y), vec2(40.0, 2.0));
            p.rect_filled(Rect::from_center_size(wave.center(), vec2(wave.width(), 2.0)), 1.0, SURFACE_HI);
            p.with_clip_rect(wave).rect_filled(shimmer, 1.0, alpha(color, 120));
            ui.ctx().request_repaint();
        }
    }
    let dur = voices.first().map(|v| v.duration - v.position).unwrap_or(s.duration / s.fx.speed.max(0.1));
    let time_txt = if playing {
        format!("-{}", fmt_time(dur))
    } else if s.duration > 0.0 {
        fmt_time(dur)
    } else {
        String::new()
    };
    p.text(
        pos2(inner.right(), wave.center().y),
        Align2::RIGHT_CENTER,
        time_txt,
        theme::body(11.5),
        if playing { color } else { TEXT_FAINT },
    );

    // hover: edit button (hit-tested against the pad's own click so it never plays the sound)
    let mut edit_clicked = false;
    if hover > 0.5 && !recording && !selected {
        let er = Rect::from_center_size(pos2(inner.right() - 12.0, inner.top() + 40.0), vec2(28.0, 28.0));
        let over = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| er.contains(p));
        ui.painter().rect_filled(er, 8.0, if over { SURFACE_HI } else { alpha(SURFACE_HI, 160) });
        ui.painter().text(
            er.center(),
            Align2::CENTER_CENTER,
            ic::SLIDERS_HORIZONTAL,
            theme::body(14.0),
            if over { TEXT } else { TEXT_DIM },
        );
        if resp.clicked() && resp.interact_pointer_pos().is_some_and(|p| er.contains(p)) {
            edit_clicked = true;
        }
    }

    // interactions
    if edit_clicked {
        app.selected = Some(s.id.clone());
        app.settings.show_inspector = true;
        app.settings_dirty = true;
    } else if resp.clicked() {
        if ui.input(|i| i.modifiers.command) {
            app.selected = Some(s.id.clone());
            app.settings.show_inspector = true;
            app.settings_dirty = true;
        } else {
            app.core.trigger(&s.id);
            if app.selected.is_some() {
                app.selected = Some(s.id.clone());
            }
        }
    }
    resp.dnd_set_drag_payload(DragSound(s.id.clone()));
    let dropped = resp.dnd_release_payload::<DragSound>().map(|d| d.0.clone());
    if resp.dnd_hover_payload::<DragSound>().is_some_and(|d| d.0 != s.id) {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.left_top() - vec2(8.0, 0.0), vec2(3.0, rect.height())),
            2.0,
            accent(),
        );
    }
    resp.context_menu(|ui| context_menu(app, ui, s));
    let tip = failed.map(|f| format!("Can't decode: {f}"));
    if let Some(t) = tip {
        resp.on_hover_text(t);
    }
    dropped
}

fn badge(p: &egui::Painter, right: f32, cy: f32, text: &str, bg: egui::Color32, fg: egui::Color32) -> f32 {
    let g = p.layout_no_wrap(text.to_string(), theme::semibold(11.0), fg);
    let r = Rect::from_min_size(pos2(right - g.size().x - 14.0, cy - 2.0), g.size() + vec2(14.0, 8.0));
    p.rect(r, 7.0, bg, Stroke::new(1.0, BORDER), egui::StrokeKind::Inside);
    p.galley(r.center() - g.size() / 2.0, g, fg);
    r.left() - 6.0
}

pub fn context_menu(app: &mut RiffApp, ui: &mut Ui, s: &Sound) {
    ui.set_min_width(200.0);
    if ui.button(format!("{}  Play", ic::PLAY)).clicked() {
        app.core.play(&s.id, true);
        ui.close();
    }
    if ui.button(format!("{}  Edit…", ic::SLIDERS_HORIZONTAL)).clicked() {
        app.selected = Some(s.id.clone());
        app.settings.show_inspector = true;
        app.settings_dirty = true;
        ui.close();
    }
    if ui.button(format!("{}  Set hotkey", ic::KEYBOARD)).clicked() {
        app.selected = Some(s.id.clone());
        app.start_recording(RecordTarget::Sound(s.id.clone()));
        ui.close();
    }
    let fav = if s.favorite { "Remove from favorites" } else { "Add to favorites" };
    if ui.button(format!("{}  {fav}", ic::STAR)).clicked() {
        app.edit_sound(&s.id, |x| x.favorite = !x.favorite);
        ui.close();
    }
    ui.menu_button(format!("{}  Move to", ic::ARROW_RIGHT), |ui| {
        for b in app.lib.boards.clone() {
            if b.id == s.board {
                continue;
            }
            if ui.button(&b.name).clicked() {
                app.edit_sound(&s.id, |x| x.board = b.id.clone());
                ui.close();
            }
        }
    });
    if ui.button(format!("{}  Duplicate", ic::COPY)).clicked() {
        app.duplicate_sound(&s.id);
        ui.close();
    }
    ui.separator();
    if ui.button(egui::RichText::new(format!("{}  Delete", ic::TRASH)).color(theme::DANGER)).clicked() {
        app.delete_sound(&s.id);
        ui.close();
    }
}

fn empty_state(app: &mut RiffApp, ui: &mut Ui) {
    let searching = !app.search.trim().is_empty();
    ui.add_space((ui.available_height() * 0.18).max(20.0));
    ui.vertical_centered(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(88.0, 88.0), Sense::hover());
        widgets::gradient_rounded_rect(ui.painter(), r, 26.0, alpha(accent(), 70), alpha(theme::color(6), 50));
        let icon = if searching { ic::MAGNIFYING_GLASS } else { ic::WAVEFORM };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, icon, theme::body(40.0), TEXT);
        ui.add_space(14.0);
        if searching {
            ui.label(egui::RichText::new("No sounds match").font(theme::display(22.0)).color(TEXT));
            ui.label(widgets::dim(format!("Nothing called “{}” here.", app.search.trim())));
            ui.add_space(10.0);
            if widgets::ghost_button(ui, ic::X, "Clear search").clicked() {
                app.search.clear();
            }
            return;
        }
        let msg = match app.board {
            BoardSel::Favorites => ("No favorites yet", "Right-click a pad and pick “Add to favorites”."),
            BoardSel::Recent => ("Nothing played yet", "Sounds you play show up here."),
            _ => ("This board is empty", "Drop audio files anywhere, or grab some from Discover and Speak."),
        };
        ui.label(egui::RichText::new(msg.0).font(theme::display(22.0)).color(TEXT));
        ui.label(widgets::dim(msg.1));
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            let w = 3.0 * 150.0;
            ui.add_space((ui.available_width() - w).max(0.0) / 2.0);
            if widgets::primary_button(ui, ic::FOLDER_OPEN, "Import files").clicked() {
                app.import_dialog();
            }
            if widgets::ghost_button(ui, ic::CHAT_CIRCLE_TEXT, "Make one with TTS").clicked() {
                app.view = View::Speak;
            }
            if widgets::ghost_button(ui, ic::COMPASS, "Discover").clicked() {
                app.view = View::Discover;
            }
        });
        ui.add_space(28.0);
        ui.label(widgets::faint("Tip: Ctrl+K opens the command palette · Esc stops everything"));
    });
    let _ = TEXT_DIM;
}
