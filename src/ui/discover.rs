//! Discover: search Freesound, preview without anyone hearing, add to a board.

use super::theme::{self, BORDER, SURFACE_HI, TEXT, TEXT_DIM, TEXT_FAINT, alpha};
use super::widgets::{self, accent, fmt_time};
use crate::app::RiffApp;
use crate::audio::clip::{Clip, decode_file};
use crate::core::ToastKind;
use crate::freesound::{self, FsSound, Length, Sort};
use crate::model::{self, Sound, SoundFx, SoundSource};
use crossbeam_channel::{Receiver, Sender};
use egui::{Align2, Sense, Stroke, Ui, vec2};
use egui_phosphor::regular as ic;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

type SearchResult = anyhow::Result<(Vec<FsSound>, u64)>;

enum Msg {
    Search(u32, SearchResult),
    Preview(u64, anyhow::Result<Arc<Clip>>),
    Added(Box<FsSound>, String, anyhow::Result<PathBuf>),
}

pub struct DiscoverState {
    query: String,
    last_query: String,
    results: Vec<FsSound>,
    count: u64,
    page: u32,
    length: Length,
    sort: Sort,
    searching: bool,
    error: Option<String>,
    previewing: Option<u64>,
    loading_preview: HashSet<u64>,
    adding: HashSet<u64>,
    added: HashSet<u64>,
    key_input: String,
    chan: (Sender<Msg>, Receiver<Msg>),
}

impl Default for DiscoverState {
    fn default() -> Self {
        Self {
            query: String::new(),
            last_query: String::new(),
            results: Vec::new(),
            count: 0,
            page: 1,
            length: Length::Short,
            sort: Sort::Relevance,
            searching: false,
            error: None,
            previewing: None,
            loading_preview: HashSet::new(),
            adding: HashSet::new(),
            added: HashSet::new(),
            key_input: String::new(),
            chan: crossbeam_channel::unbounded(),
        }
    }
}

impl DiscoverState {
    pub fn busy(&self) -> bool {
        self.searching || !self.loading_preview.is_empty() || !self.adding.is_empty()
    }
}

const IDEAS: &[&str] = &[
    "airhorn",
    "bruh",
    "applause",
    "drum roll",
    "vine boom",
    "wow",
    "laugh",
    "fail",
    "door knock",
    "notification",
    "rizz",
    "explosion",
];

pub fn show(app: &mut RiffApp, ui: &mut Ui) {
    poll(app);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Discover").font(theme::display(28.0)).color(TEXT));
            ui.label(widgets::dim(
                "Half a million free sounds from Freesound.org — preview privately, add with one click.",
            ));
        });
    });
    ui.add_space(14.0);

    if app.settings.freesound_key.trim().is_empty() {
        connect_card(app, ui);
        return;
    }

    search_bar(app, ui);
    ui.add_space(12.0);

    let d = &app.discover;
    if let Some(e) = &d.error {
        ui.label(egui::RichText::new(format!("{}  {e}", ic::WARNING)).color(theme::DANGER));
        return;
    }
    if d.results.is_empty() {
        if d.searching {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().color(accent()));
                ui.label(widgets::dim("Searching Freesound…"));
            });
        } else {
            ideas(app, ui);
        }
        return;
    }
    ui.label(widgets::faint(format!("{} results for “{}”", d.count, d.last_query)));
    ui.add_space(6.0);
    let results = d.results.clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for r in &results {
            result_row(app, ui, r);
        }
        let shown = app.discover.page as u64 * 30;
        if shown < app.discover.count {
            ui.add_space(6.0);
            ui.vertical_centered(|ui| {
                if app.discover.searching {
                    ui.add(egui::Spinner::new().color(accent()));
                } else if widgets::ghost_button(ui, ic::ARROW_RIGHT, "Load more").clicked() {
                    let page = app.discover.page + 1;
                    search(app, page);
                }
            });
        }
        ui.add_space(20.0);
    });
}

fn connect_card(app: &mut RiffApp, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        widgets::section_title(
            ui,
            ic::COMPASS,
            "Connect Freesound",
            "Free, takes about a minute — your key stays on this computer",
        );
        for (n, step) in [
            "Create a free account (or log in) at freesound.org",
            "Open the API page and apply for a key — any name and description work, e.g. “Riff soundboard”",
            "Copy the long “Client secret / Api key” value and paste it below",
        ]
        .iter()
        .enumerate()
        {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 11.0, alpha(accent(), 40));
                ui.painter().text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    (n + 1).to_string(),
                    theme::semibold(12.0),
                    accent(),
                );
                ui.label(egui::RichText::new(*step).color(TEXT_DIM));
            });
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if widgets::ghost_button(ui, ic::ARROW_SQUARE_OUT, "Get an API key").clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(freesound::API_KEY_URL));
            }
            ui.add(
                egui::TextEdit::singleline(&mut app.discover.key_input)
                    .hint_text("Paste your API key")
                    .password(true)
                    .desired_width(280.0),
            );
            if widgets::primary_button(ui, ic::CHECK, "Connect").clicked() && !app.discover.key_input.trim().is_empty()
            {
                app.settings.freesound_key = app.discover.key_input.trim().to_string();
                app.settings_dirty = true;
                app.discover.key_input.clear();
                app.toast(ToastKind::Success, "Freesound connected");
            }
        });
    });
}

fn search_bar(app: &mut RiffApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        let r = widgets::search_field(
            ui,
            &mut app.discover.query,
            "Search sounds — e.g. airhorn, bruh, applause",
            (ui.available_width() - 330.0).max(160.0),
            egui::Id::new("fs-search"),
        );
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if widgets::primary_button(ui, ic::MAGNIFYING_GLASS, "Search").clicked() || enter {
            search(app, 1);
        }
        let mut len = app.discover.length;
        if widgets::segmented(ui, &mut len, &[(Length::Short, "≤ 5s"), (Length::Medium, "≤ 30s"), (Length::Any, "Any")])
        {
            app.discover.length = len;
            if !app.discover.last_query.is_empty() {
                search(app, 1);
            }
        }
        let mut sort = app.discover.sort;
        let label = match sort {
            Sort::Relevance => "Best match",
            Sort::Downloads => "Popular",
            Sort::Rating => "Top rated",
        };
        egui::ComboBox::from_id_salt("fs-sort").selected_text(label).width(110.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut sort, Sort::Relevance, "Best match");
            ui.selectable_value(&mut sort, Sort::Downloads, "Popular");
            ui.selectable_value(&mut sort, Sort::Rating, "Top rated");
        });
        if sort != app.discover.sort {
            app.discover.sort = sort;
            if !app.discover.last_query.is_empty() {
                search(app, 1);
            }
        }
    });
}

fn ideas(app: &mut RiffApp, ui: &mut Ui) {
    ui.add_space(30.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(ic::SPARKLE).size(36.0).color(accent()));
        ui.label(egui::RichText::new("What are you looking for?").font(theme::display(20.0)).color(TEXT));
        ui.add_space(8.0);
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let w: f32 = 560.0;
        ui.add_space(((ui.available_width() - w) / 2.0).max(0.0));
        for idea in IDEAS {
            if widgets::chip(ui, idea, TEXT_DIM)
                .interact(Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                app.discover.query = idea.to_string();
                search(app, 1);
            }
        }
    });
}

fn search(app: &mut RiffApp, page: u32) {
    let d = &mut app.discover;
    let q = d.query.trim().to_string();
    if q.is_empty() {
        return;
    }
    d.searching = true;
    d.error = None;
    d.last_query = q.clone();
    if page == 1 {
        d.results.clear();
    }
    let key = app.settings.freesound_key.clone();
    let (len, sort) = (d.length, d.sort);
    let tx = d.chan.0.clone();
    let core = app.core.clone();
    std::thread::spawn(move || {
        let r = freesound::search(&key, &q, len, sort, page);
        let _ = tx.send(Msg::Search(page, r));
        core.repaint();
    });
}

fn poll(app: &mut RiffApp) {
    let msgs: Vec<Msg> = app.discover.chan.1.try_iter().collect();
    for m in msgs {
        match m {
            Msg::Search(page, r) => {
                let d = &mut app.discover;
                d.searching = false;
                match r {
                    Ok((list, count)) => {
                        d.page = page;
                        d.count = count;
                        if page == 1 {
                            d.results = list;
                        } else {
                            d.results.extend(list);
                        }
                    }
                    Err(e) => d.error = Some(format!("{e:#}")),
                }
            }
            Msg::Preview(id, r) => {
                app.discover.loading_preview.remove(&id);
                match r {
                    Ok(clip) => {
                        app.core.stop("preview");
                        app.core.play_adhoc("preview", clip, SoundFx { to_mic: false, ..Default::default() });
                        app.discover.previewing = Some(id);
                    }
                    Err(e) => app.toast(ToastKind::Error, format!("Preview failed: {e:#}")),
                }
            }
            Msg::Added(s, board, r) => {
                app.discover.adding.remove(&s.id);
                match r {
                    Ok(path) => {
                        let name = s.clean_name();
                        app.add_sound(Sound {
                            emoji: model::guess_emoji(&name).into(),
                            color: fastrand::usize(..theme::PALETTE.len()),
                            name: name.clone(),
                            path,
                            board,
                            duration: s.duration,
                            source: SoundSource::Freesound {
                                id: s.id,
                                username: s.username.clone(),
                                license: s.license_short().into(),
                                url: s.url.clone(),
                            },
                            ..Default::default()
                        });
                        app.discover.added.insert(s.id);
                        app.toast(ToastKind::Success, format!("Added “{name}”"));
                    }
                    Err(e) => app.toast(ToastKind::Error, format!("Couldn't add: {e:#}")),
                }
            }
        }
    }
    // clear the preview marker once it finishes
    if app.discover.previewing.is_some()
        && !app.status.voices.iter().any(|v| v.sound_id == "preview")
        && app.discover.loading_preview.is_empty()
    {
        app.discover.previewing = None;
    }
}

fn result_row(app: &mut RiffApp, ui: &mut Ui, r: &FsSound) {
    let previewing = app.discover.previewing == Some(r.id);
    let loading = app.discover.loading_preview.contains(&r.id);
    let progress =
        previewing.then(|| app.status.voices.iter().find(|v| v.sound_id == "preview").map(|v| v.progress)).flatten();
    egui::Frame::new()
        .fill(if previewing { alpha(accent(), 18) } else { theme::SURFACE })
        .stroke(Stroke::new(1.0, if previewing { alpha(accent(), 90) } else { BORDER }))
        .corner_radius(14)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                // play button
                let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
                let h = resp.hovered();
                ui.painter().circle_filled(
                    rect.center(),
                    20.0,
                    if previewing {
                        accent()
                    } else if h {
                        SURFACE_HI
                    } else {
                        theme::SURFACE_LO
                    },
                );
                if let Some(p) = progress {
                    let pts: Vec<_> = (0..=(40.0 * p) as usize)
                        .map(|i| {
                            let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / 40.0;
                            rect.center() + vec2(a.cos(), a.sin()) * 20.0
                        })
                        .collect();
                    if pts.len() > 1 {
                        ui.painter().add(egui::Shape::line(pts, Stroke::new(2.0, egui::Color32::WHITE)));
                    }
                }
                if loading {
                    ui.put(rect, egui::Spinner::new().size(16.0).color(accent()));
                } else {
                    let icon = if previewing { ic::STOP } else { ic::PLAY };
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon, theme::body(16.0), TEXT);
                }
                if resp
                    .on_hover_text("Preview (only you hear it)")
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                    && !loading
                {
                    if previewing {
                        app.core.stop("preview");
                        app.discover.previewing = None;
                    } else {
                        app.discover.loading_preview.insert(r.id);
                        app.discover.previewing = Some(r.id);
                        let s = r.clone();
                        let tx = app.discover.chan.0.clone();
                        let core = app.core.clone();
                        std::thread::spawn(move || {
                            let res = freesound::fetch_preview(&s).and_then(|p| decode_file(&p)).map(Arc::new);
                            let _ = tx.send(Msg::Preview(s.id, res));
                            core.repaint();
                        });
                    }
                }

                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.set_width((ui.available_width() - 150.0).max(100.0));
                    ui.label(egui::RichText::new(r.clean_name()).font(theme::semibold(14.0)).color(TEXT));
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(widgets::faint(format!(
                            "{} · {} · {} downloads",
                            fmt_time(r.duration),
                            r.username,
                            r.num_downloads
                        )));
                        let lc = if r.license_short() == "CC0" {
                            theme::OK
                        } else if r.license_short().contains("NC") {
                            theme::WARN
                        } else {
                            TEXT_DIM
                        };
                        widgets::chip(ui, r.license_short(), lc).on_hover_text(&r.license);
                        for t in r.tags.iter().take(3) {
                            widgets::chip(ui, t, TEXT_FAINT);
                        }
                    });
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if app.discover.added.contains(&r.id) {
                        widgets::chip(ui, &format!("{} Added", ic::CHECK), theme::OK);
                    } else if app.discover.adding.contains(&r.id) {
                        ui.add(egui::Spinner::new().color(accent()));
                    } else {
                        let board = app.current_board_id();
                        let bname = app.lib.board(&board).map(|b| b.name.clone()).unwrap_or_default();
                        if widgets::primary_button(ui, ic::PLUS, "Add")
                            .on_hover_text(format!("Add to {bname}"))
                            .clicked()
                        {
                            app.discover.adding.insert(r.id);
                            let s = r.clone();
                            let tx = app.discover.chan.0.clone();
                            let core = app.core.clone();
                            std::thread::spawn(move || {
                                let res = freesound::fetch_preview(&s)
                                    .and_then(|p| model::adopt_file(&p, true).map_err(Into::into));
                                let _ = tx.send(Msg::Added(Box::new(s), board, res));
                                core.repaint();
                            });
                        }
                    }
                    if ui.small_button(ic::ARROW_SQUARE_OUT).on_hover_text("Open on freesound.org").clicked()
                        && !r.url.is_empty()
                    {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(&r.url));
                    }
                });
            });
        });
    ui.add_space(6.0);
}
