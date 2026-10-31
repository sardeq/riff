//! Ctrl+K command palette: fuzzy-find any sound, board or action.

use super::theme::{self, BORDER, SURFACE_HI, TEXT, TEXT_DIM, TEXT_FAINT, alpha};
use super::widgets::accent;
use crate::app::{BoardSel, RiffApp, View};
use egui::{Align2, Rect, Sense, Stroke, pos2, vec2};
use egui_phosphor::regular as ic;
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

#[derive(Default)]
pub struct PaletteState {
    pub open: bool,
    query: String,
    cursor: usize,
    just_opened: bool,
}

impl PaletteState {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.query.clear();
        self.cursor = 0;
        self.just_opened = true;
    }
}

#[derive(Clone)]
enum Item {
    Sound { id: String, name: String, emoji: String, color: usize, hint: String },
    Board(String, String),
    Action(&'static str, &'static str, Act),
}

#[derive(Clone, Copy)]
enum Act {
    StopAll,
    Random,
    ToggleMic,
    Import,
    NewBoard,
    Go(View),
}

fn items(app: &RiffApp) -> Vec<Item> {
    let mut v: Vec<Item> = vec![
        Item::Action(ic::STOP, "Stop all sounds", Act::StopAll),
        Item::Action(ic::SHUFFLE, "Play a random sound", Act::Random),
        Item::Action(ic::MICROPHONE, "Toggle my microphone", Act::ToggleMic),
        Item::Action(ic::FOLDER_OPEN, "Import sounds…", Act::Import),
        Item::Action(ic::PLUS, "New board", Act::NewBoard),
        Item::Action(ic::CHAT_CIRCLE_TEXT, "Go to Speak (text to speech)", Act::Go(View::Speak)),
        Item::Action(ic::COMPASS, "Go to Discover (Freesound)", Act::Go(View::Discover)),
        Item::Action(ic::GEAR_SIX, "Open Settings", Act::Go(View::Settings)),
    ];
    for b in &app.lib.boards {
        v.push(Item::Board(b.id.clone(), b.name.clone()));
    }
    for s in &app.lib.sounds {
        let board = app.lib.board(&s.board).map(|b| b.name.as_str()).unwrap_or("");
        let hint = match &s.hotkey {
            Some(h) => format!("{board} · {}", h.label()),
            None => board.to_string(),
        };
        v.push(Item::Sound { id: s.id.clone(), name: s.name.clone(), emoji: s.emoji.clone(), color: s.color, hint });
    }
    v
}

fn label(i: &Item) -> String {
    match i {
        Item::Sound { name, .. } => name.clone(),
        Item::Board(_, n) => format!("Board: {n}"),
        Item::Action(_, n, _) => n.to_string(),
    }
}

pub fn show(app: &mut RiffApp, ctx: &egui::Context) {
    if !app.palette.open {
        return;
    }
    let screen = ctx.content_rect();
    // backdrop
    let bg =
        egui::Area::new(egui::Id::new("palette-bg")).order(egui::Order::Middle).fixed_pos(screen.min).show(ctx, |ui| {
            let (r, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(r, 0.0, egui::Color32::from_black_alpha(140));
            resp
        });
    if bg.inner.clicked() {
        app.palette.open = false;
        return;
    }

    let all = items(app);
    let matcher = SkimMatcherV2::default();
    let q = app.palette.query.trim().to_string();
    let mut scored: Vec<(i64, Item)> = if q.is_empty() {
        // sounds first by popularity, then actions
        let mut s: Vec<(i64, Item)> = all
            .iter()
            .filter(|i| matches!(i, Item::Sound { .. }))
            .map(|i| {
                let plays = match i {
                    Item::Sound { id, .. } => app.lib.sound(id).map(|s| s.plays as i64).unwrap_or(0),
                    _ => 0,
                };
                (plays, i.clone())
            })
            .collect();
        s.sort_by_key(|(p, _)| std::cmp::Reverse(*p));
        s.truncate(6);
        s.extend(all.iter().filter(|i| matches!(i, Item::Action(..))).map(|i| (0, i.clone())));
        s
    } else {
        let mut s: Vec<(i64, Item)> =
            all.into_iter().filter_map(|i| matcher.fuzzy_match(&label(&i), &q).map(|sc| (sc, i))).collect();
        s.sort_by_key(|(sc, _)| std::cmp::Reverse(*sc));
        s
    };
    scored.truncate(9);
    let n = scored.len();

    let (up, down, enter, esc) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if esc {
        app.palette.open = false;
        return;
    }
    if n > 0 {
        if down {
            app.palette.cursor = (app.palette.cursor + 1) % n;
        }
        if up {
            app.palette.cursor = (app.palette.cursor + n - 1) % n;
        }
        app.palette.cursor = app.palette.cursor.min(n - 1);
    }

    let width = 560.0f32.min(screen.width() - 40.0);
    let mut chosen: Option<Item> = None;
    egui::Area::new(egui::Id::new("palette"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(screen.center().x - width / 2.0, screen.top() + screen.height() * 0.16))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme::SURFACE)
                .stroke(Stroke::new(1.0, BORDER))
                .corner_radius(18)
                .shadow(egui::Shadow {
                    offset: [0, 20],
                    blur: 50,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(140),
                })
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(ic::MAGNIFYING_GLASS).size(18.0).color(TEXT_FAINT));
                        let before = app.palette.query.clone();
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut app.palette.query)
                                .hint_text("Play a sound, jump to a board, run a command…")
                                .font(theme::body(17.0))
                                .frame(egui::Frame::NONE)
                                .desired_width(f32::INFINITY),
                        );
                        if app.palette.just_opened || !r.has_focus() {
                            r.request_focus();
                            app.palette.just_opened = false;
                        }
                        if before != app.palette.query {
                            app.palette.cursor = 0;
                        }
                    });
                    ui.add_space(6.0);
                    ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, BORDER));
                    ui.add_space(6.0);
                    if n == 0 {
                        ui.add_space(8.0);
                        ui.vertical_centered(|ui| ui.label(egui::RichText::new("No matches").color(TEXT_DIM)));
                        ui.add_space(8.0);
                    }
                    for (i, (_, item)) in scored.iter().enumerate() {
                        let sel = i == app.palette.cursor;
                        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::click());
                        if resp.hovered() && ui.input(|i| i.pointer.delta().length_sq() > 0.0) {
                            app.palette.cursor = i;
                        }
                        if sel {
                            ui.painter().rect_filled(rect, 10.0, alpha(accent(), 34));
                        } else if resp.hovered() {
                            ui.painter().rect_filled(rect, 10.0, SURFACE_HI);
                        }
                        let (icon, icon_col, title, hint) = match item {
                            Item::Sound { emoji, color, name, hint, .. } => {
                                (emoji.clone(), theme::color(*color), name.clone(), hint.clone())
                            }
                            Item::Board(_, n) => (ic::SQUARES_FOUR.to_string(), TEXT_DIM, n.clone(), "Board".into()),
                            Item::Action(icn, n, _) => (icn.to_string(), accent(), n.to_string(), "Command".into()),
                        };
                        let ib = Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), vec2(28.0, 28.0));
                        ui.painter().rect_filled(ib, 8.0, alpha(icon_col, 36));
                        ui.painter().text(
                            ib.center(),
                            Align2::CENTER_CENTER,
                            icon,
                            theme::body(15.0),
                            theme::mix(icon_col, egui::Color32::WHITE, 0.3),
                        );
                        ui.painter().text(
                            pos2(rect.left() + 46.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            title,
                            theme::semibold(14.0),
                            TEXT,
                        );
                        ui.painter().text(
                            rect.right_center() - vec2(12.0, 0.0),
                            Align2::RIGHT_CENTER,
                            hint,
                            theme::body(12.0),
                            TEXT_FAINT,
                        );
                        if resp.clicked() || (sel && enter) {
                            chosen = Some(item.clone());
                        }
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("↑↓ navigate · Enter run · Esc close").size(11.5).color(TEXT_FAINT),
                        );
                    });
                });
        });

    if let Some(item) = chosen {
        app.palette.open = false;
        match item {
            Item::Sound { id, .. } => app.core.trigger(&id),
            Item::Board(id, _) => {
                app.board = BoardSel::Board(id);
                app.view = View::Sounds;
            }
            Item::Action(_, _, act) => match act {
                Act::StopAll => app.core.stop_all(),
                Act::Random => {
                    app.core.random(None);
                }
                Act::ToggleMic => {
                    let v = app.core.toggle_mic(None);
                    app.settings.mic_passthrough = v;
                }
                Act::Import => app.import_dialog(),
                Act::NewBoard => app.new_board(),
                Act::Go(v) => app.view = v,
            },
        }
    }
}
