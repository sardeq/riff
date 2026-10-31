use super::theme::{self, SURFACE_HI, TEXT, TEXT_DIM, TEXT_FAINT, alpha};
use super::widgets::{self, accent, label_caps};
use crate::app::{BoardSel, RiffApp, View};
use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular as ic;

/// Payload for dragging pads onto boards.
#[derive(Clone)]
pub struct DragSound(pub String);

pub fn show(app: &mut RiffApp, ui: &mut Ui) {
    logo(app, ui);
    ui.add_space(18.0);

    for (view, icon, label) in [
        (View::Sounds, ic::SQUARES_FOUR, "Sounds"),
        (View::Speak, ic::CHAT_CIRCLE_TEXT, "Speak"),
        (View::Discover, ic::COMPASS, "Discover"),
        (View::Settings, ic::GEAR_SIX, "Settings"),
    ] {
        if nav_item(ui, icon, label, None, app.view == view, None).clicked() {
            app.view = view;
        }
    }

    ui.add_space(16.0);
    label_caps(ui, "Library");
    ui.add_space(2.0);
    let total = app.lib.sounds.len();
    let favs = app.lib.sounds.iter().filter(|s| s.favorite).count();
    for (sel, icon, label, count) in [
        (BoardSel::All, ic::STACK, "All sounds", total),
        (BoardSel::Favorites, ic::STAR, "Favorites", favs),
        (BoardSel::Recent, ic::CLOCK_COUNTER_CLOCKWISE, "Recent", 0),
    ] {
        let active = app.view == View::Sounds && app.board == sel;
        let c = (count > 0).then_some(count);
        if nav_item(ui, icon, label, c, active, None).clicked() {
            app.board = sel;
            app.view = View::Sounds;
        }
    }

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        label_caps(ui, "Boards");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::small_icon_button(ui, ic::PLUS, "New board").clicked() {
                app.new_board();
            }
        });
    });

    let boards = app.lib.boards.clone();
    let reserve = 92.0;
    egui::ScrollArea::vertical()
        .max_height((ui.available_height() - reserve).max(60.0))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for b in &boards {
                let count = app.lib.sounds.iter().filter(|s| s.board == b.id).count();
                let sel = BoardSel::Board(b.id.clone());
                let active = app.view == View::Sounds && app.board == sel;

                if let Some((rid, text)) = app.renaming_board.as_mut()
                    && *rid == b.id
                {
                    let r = ui.add(egui::TextEdit::singleline(text).hint_text(&b.name).desired_width(f32::INFINITY));
                    r.request_focus();
                    if r.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let (id, name) = app.renaming_board.take().unwrap();
                        let name = name.trim().to_string();
                        if !name.is_empty() {
                            app.edit_lib(|lib| {
                                if let Some(bb) = lib.boards.iter_mut().find(|x| x.id == id) {
                                    bb.name = name;
                                }
                            });
                        }
                    }
                    continue;
                }

                let resp = nav_item(ui, "", &b.name, Some(count), active, Some(theme::color(b.color)));
                // drop pads here to move them
                if let Some(payload) = resp.dnd_release_payload::<DragSound>() {
                    let target = b.id.clone();
                    let name = b.name.clone();
                    app.edit_sound(&payload.0, |s| s.board = target);
                    app.toast(crate::core::ToastKind::Success, format!("Moved to {name}"));
                }
                if resp.clicked() {
                    app.board = sel;
                    app.view = View::Sounds;
                    app.settings.last_board = b.id.clone();
                    app.settings_dirty = true;
                }
                if resp.double_clicked() {
                    app.renaming_board = Some((b.id.clone(), String::new()));
                }
                resp.context_menu(|ui| {
                    if ui.button(format!("{}  Rename", ic::PENCIL_SIMPLE)).clicked() {
                        app.renaming_board = Some((b.id.clone(), String::new()));
                        ui.close();
                    }
                    ui.menu_button(format!("{}  Color", ic::PALETTE), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.set_max_width(150.0);
                            for (i, (c, name)) in theme::PALETTE.iter().enumerate() {
                                let (r, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                                ui.painter().circle_filled(r.center(), 9.0, *c);
                                if resp.on_hover_text(*name).clicked() {
                                    let id = b.id.clone();
                                    app.edit_lib(|lib| {
                                        if let Some(bb) = lib.boards.iter_mut().find(|x| x.id == id) {
                                            bb.color = i;
                                        }
                                    });
                                    ui.close();
                                }
                            }
                        });
                    });
                    if ui.button(format!("{}  Play random", ic::SHUFFLE)).clicked() {
                        app.core.random(Some(&b.id));
                        ui.close();
                    }
                    ui.separator();
                    let can_delete = app.lib.boards.len() > 1;
                    if ui
                        .add_enabled(
                            can_delete,
                            egui::Button::new(
                                egui::RichText::new(format!("{}  Delete board", ic::TRASH)).color(theme::DANGER),
                            ),
                        )
                        .clicked()
                    {
                        let id = b.id.clone();
                        app.edit_lib(|lib| lib.remove_board(&id));
                        app.board = BoardSel::Board(app.lib.boards[0].id.clone());
                        app.toast(crate::core::ToastKind::Info, format!("Deleted board {} ({count} sounds)", b.name));
                        ui.close();
                    }
                });
            }
        });

    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        mic_status(app, ui);
    });
}

fn logo(app: &RiffApp, ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
    let tile = Rect::from_min_size(rect.min, vec2(36.0, 36.0));
    let a = accent();
    let b = theme::color(6);
    widgets::gradient_rounded_rect(ui.painter(), tile, 10.0, a, b);

    // equaliser bars dance while anything plays
    let t = ui.input(|i| i.time) as f32;
    let playing = !app.status.voices.is_empty();
    let lvl = app.status.peak_monitor.max(app.status.peak_mic_bus).clamp(0.0, 1.0);
    for (i, base) in [0.42f32, 0.7, 0.52].iter().enumerate() {
        let h = if playing {
            (base * (0.55 + 0.45 * (t * (7.0 + i as f32 * 2.3)).sin().abs()) * (0.6 + lvl * 0.6)).clamp(0.2, 0.8)
        } else {
            *base
        };
        let x = tile.left() + tile.width() * (0.3 + 0.17 * i as f32);
        let hh = tile.height() * h;
        let r = Rect::from_center_size(pos2(x, tile.center().y), vec2(4.0, hh));
        ui.painter().rect_filled(r, 2.0, egui::Color32::WHITE);
    }

    ui.painter().text(
        pos2(tile.right() + 12.0, rect.center().y - 1.0),
        Align2::LEFT_CENTER,
        "riff",
        theme::display(26.0),
        TEXT,
    );
    ui.painter().text(
        pos2(rect.right(), rect.center().y),
        Align2::RIGHT_CENTER,
        concat!("v", env!("CARGO_PKG_VERSION")),
        theme::body(11.0),
        TEXT_FAINT,
    );
}

fn nav_item(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    count: Option<usize>,
    active: bool,
    dot: Option<egui::Color32>,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    let hover = ui.ctx().animate_bool_responsive(resp.id, resp.hovered() || active);
    let drop_hover = dot.is_some() && resp.dnd_hover_payload::<DragSound>().is_some();
    if active {
        ui.painter().rect_filled(rect, 10.0, alpha(accent(), 34));
        ui.painter().rect_filled(Rect::from_min_size(rect.left_top() + vec2(0.0, 9.0), vec2(3.0, 16.0)), 2.0, accent());
    } else if hover > 0.0 {
        ui.painter().rect_filled(rect, 10.0, alpha(SURFACE_HI, (hover * 160.0) as u8));
    }
    if drop_hover {
        ui.painter().rect_stroke(rect, 10.0, Stroke::new(1.5, accent()), egui::StrokeKind::Inside);
    }
    let fg = if active { TEXT } else { theme::mix(TEXT_DIM, TEXT, hover) };
    let mut x = rect.left() + 14.0;
    if let Some(c) = dot {
        ui.painter().circle_filled(pos2(x + 5.0, rect.center().y), 4.5, c);
        x += 22.0;
    } else if !icon.is_empty() {
        ui.painter().text(
            pos2(x, rect.center().y),
            Align2::LEFT_CENTER,
            icon,
            theme::body(17.0),
            if active { accent() } else { fg },
        );
        x += 28.0;
    }
    let font = if active { theme::semibold(14.0) } else { theme::body(14.0) };
    let galley = ui.painter().layout(label.to_string(), font, fg, rect.right() - x - 36.0);
    let galley = if galley.rows.len() > 1 {
        let mut job =
            egui::text::LayoutJob::single_section(label.to_string(), egui::TextFormat::simple(theme::body(14.0), fg));
        job.wrap = egui::text::TextWrapping {
            max_width: rect.right() - x - 36.0,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
        ui.painter().layout_job(job)
    } else {
        galley
    };
    ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    if let Some(c) = count {
        ui.painter().text(
            rect.right_center() - vec2(12.0, 0.0),
            Align2::RIGHT_CENTER,
            c.to_string(),
            theme::body(12.0),
            TEXT_FAINT,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn mic_status(app: &mut RiffApp, ui: &mut Ui) {
    let st = &app.status;
    let (dot, title, sub) = if st.virtual_ok {
        (theme::OK, "Riff Microphone", "Live · pick it as your input")
    } else if !app.settings.virtual_mic {
        (TEXT_FAINT, "Speakers only", "Virtual mic is off")
    } else if st.output_ok {
        (theme::WARN, "Virtual mic offline", "Playing to speakers")
    } else {
        (theme::DANGER, "No audio output", "Check Settings")
    };
    let resp = egui::Frame::new()
        .fill(theme::SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER))
        .corner_radius(14)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 15.0, alpha(dot, 36));
                let pulse = if st.virtual_ok { 0.6 + 0.4 * st.peak_mic_bus.clamp(0.0, 1.0) } else { 0.6 };
                ui.painter().text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    ic::MICROPHONE,
                    theme::body(16.0),
                    theme::mix(dot, egui::Color32::WHITE, 1.0 - pulse),
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(egui::RichText::new(title).font(theme::semibold(13.0)).color(TEXT));
                    ui.label(egui::RichText::new(sub).size(11.5).color(TEXT_DIM));
                });
            });
        })
        .response
        .interact(Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        app.view = View::Settings;
    }
}
