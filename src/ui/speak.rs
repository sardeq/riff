//! Text-to-speech view: say things into the mic, keep the good ones as pads.

use super::theme::{self, BORDER, SURFACE_HI, TEXT, TEXT_DIM, TEXT_FAINT, alpha};
use super::widgets::{self, accent};
use crate::app::RiffApp;
use crate::audio::clip::Clip;
use crate::core::ToastKind;
use crate::model::{Sound, SoundFx, SoundSource, TtsEngine, TtsSettings};
use crate::tts::{self, EspeakVoice, PiperVoice};
use crossbeam_channel::{Receiver, Sender};
use egui::{Align2, Sense, Stroke, Ui, vec2};
use egui_phosphor::regular as ic;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

const QUICK: &[&str] = &[
    "gg",
    "Be right back",
    "Let's go!",
    "Nice shot!",
    "One sec",
    "I'm muted, typing instead",
    "Good morning everyone",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Speak,
    Preview,
    Save,
}

#[derive(Clone)]
struct Line {
    text: String,
    voice: String,
    clip: Arc<Clip>,
    wav: Arc<Vec<u8>>,
}

type DownloadDone = (String, anyhow::Result<()>);

#[derive(Default)]
pub struct SpeakState {
    text: String,
    history: Vec<Line>,
    job: Option<Receiver<(Mode, String, anyhow::Result<tts::Speech>)>>,
    catalog: Option<Result<Vec<PiperVoice>, String>>,
    catalog_job: Option<Receiver<anyhow::Result<Vec<PiperVoice>>>>,
    downloads: HashMap<String, Arc<AtomicU32>>,
    done: Option<(Sender<DownloadDone>, Receiver<DownloadDone>)>,
    filter: String,
    espeak_voices: Option<Vec<EspeakVoice>>,
    installed: Option<Vec<String>>,
    bins: Option<(bool, bool)>,
}

impl SpeakState {
    pub fn busy(&self) -> bool {
        self.job.is_some() || self.catalog_job.is_some() || !self.downloads.is_empty()
    }
}

pub fn show(app: &mut RiffApp, ui: &mut Ui) {
    poll(app);
    let st = &mut app.speak;
    if st.bins.is_none() {
        let t = &app.settings.tts;
        st.bins = Some((tts::which(&t.piper_bin), tts::which(&t.espeak_bin)));
    }
    if st.installed.is_none() {
        st.installed = Some(tts::installed_voices());
    }

    ui.label(egui::RichText::new("Speak").font(theme::display(28.0)).color(TEXT));
    ui.label(widgets::dim("Type it, Riff says it into your mic. Everything runs locally."));
    ui.add_space(14.0);

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let wide = ui.available_width() > 900.0;
        if wide {
            ui.horizontal_top(|ui| {
                let w = ui.available_width();
                let top = egui::Layout::top_down(egui::Align::Min);
                ui.allocate_ui_with_layout(vec2(w * 0.58, 0.0), top, |ui| {
                    ui.set_width(w * 0.58);
                    compose(app, ui);
                    ui.add_space(14.0);
                    history(app, ui);
                });
                ui.add_space(6.0);
                let rest = ui.available_width();
                ui.allocate_ui_with_layout(vec2(rest, 0.0), top, |ui| {
                    ui.set_width(rest);
                    voice_card(app, ui)
                });
            });
        } else {
            compose(app, ui);
            ui.add_space(14.0);
            voice_card(app, ui);
            ui.add_space(14.0);
            history(app, ui);
        }
        ui.add_space(20.0);
    });
}

fn poll(app: &mut RiffApp) {
    if let Some(rx) = &app.speak.job
        && let Ok((mode, text, res)) = rx.try_recv()
    {
        app.speak.job = None;
        match res {
            Ok(sp) => {
                let line = Line {
                    text: text.clone(),
                    voice: sp.voice_label.clone(),
                    clip: Arc::new(sp.clip),
                    wav: Arc::new(sp.wav),
                };
                match mode {
                    Mode::Speak => app.core.play_adhoc("tts", line.clip.clone(), SoundFx::default()),
                    Mode::Preview => {
                        app.core.play_adhoc("tts", line.clip.clone(), SoundFx { to_mic: false, ..Default::default() })
                    }
                    Mode::Save => save_line(app, &line),
                }
                app.speak.history.retain(|l| !(l.text == line.text && l.voice == line.voice));
                app.speak.history.insert(0, line);
                app.speak.history.truncate(25);
            }
            Err(e) => app.toast(ToastKind::Error, format!("{e:#}")),
        }
    }
    if let Some(rx) = &app.speak.catalog_job
        && let Ok(res) = rx.try_recv()
    {
        app.speak.catalog_job = None;
        app.speak.catalog = Some(res.map_err(|e| format!("{e:#}")));
    }
    let mut finished = Vec::new();
    if let Some((_, rx)) = &app.speak.done {
        while let Ok(r) = rx.try_recv() {
            finished.push(r);
        }
    }
    for (key, res) in finished {
        app.speak.downloads.remove(&key);
        app.speak.installed = None;
        match res {
            Ok(()) => {
                app.toast(ToastKind::Success, format!("Voice {key} ready"));
                if !tts::is_installed(&app.settings.tts.piper_voice) {
                    app.settings.tts.piper_voice = key;
                    app.settings_dirty = true;
                }
            }
            Err(e) => app.toast(ToastKind::Error, format!("Download failed: {e:#}")),
        }
    }
}

fn run(app: &mut RiffApp, mode: Mode, text: String) {
    if text.trim().is_empty() || app.speak.job.is_some() {
        return;
    }
    // replay from history when nothing changed — instant, no re-synthesis
    let voice_key = voice_label(&app.settings.tts);
    if let Some(l) = app.speak.history.iter().find(|l| l.text == text && l.voice == voice_key).cloned() {
        match mode {
            Mode::Speak => app.core.play_adhoc("tts", l.clip.clone(), SoundFx::default()),
            Mode::Preview => {
                app.core.play_adhoc("tts", l.clip.clone(), SoundFx { to_mic: false, ..Default::default() })
            }
            Mode::Save => save_line(app, &l),
        }
        return;
    }
    let (tx, rx) = crossbeam_channel::bounded(1);
    let settings = app.settings.tts.clone();
    let core = app.core.clone();
    std::thread::spawn(move || {
        let r = tts::synthesize(&settings, &text);
        let _ = tx.send((mode, text, r));
        core.repaint();
    });
    app.speak.job = Some(rx);
}

fn voice_label(t: &TtsSettings) -> String {
    match t.engine {
        TtsEngine::Piper => format!("piper · {}", t.piper_voice),
        TtsEngine::Espeak => {
            if t.espeak_variant.is_empty() {
                format!("espeak · {}", t.espeak_voice)
            } else {
                format!("espeak · {}+{}", t.espeak_voice, t.espeak_variant)
            }
        }
    }
}

fn save_line(app: &mut RiffApp, l: &Line) {
    match tts::save_wav(&l.wav) {
        Ok(path) => {
            let mut name: String = l.text.chars().take(32).collect();
            if l.text.chars().count() > 32 {
                name.push('…');
            }
            let board = app.current_board_id();
            app.add_sound(Sound {
                name,
                emoji: "🤖".into(),
                color: 5,
                path,
                board,
                duration: l.clip.duration(),
                source: SoundSource::Tts { text: l.text.clone(), voice: l.voice.clone() },
                ..Default::default()
            });
            app.toast(ToastKind::Success, "Saved as a pad on your board");
        }
        Err(e) => app.toast(ToastKind::Error, format!("Couldn't save: {e}")),
    }
}

fn compose(app: &mut RiffApp, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        let busy = app.speak.job.is_some();
        let edit = egui::TextEdit::multiline(&mut app.speak.text)
            .hint_text("Type something to say into the mic…")
            .font(theme::body(17.0))
            .desired_rows(4)
            .desired_width(f32::INFINITY)
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE_LO)
                    .corner_radius(12)
                    .inner_margin(12)
                    .stroke(Stroke::new(1.0, BORDER)),
            );
        let r = ui.add(edit);
        let send = r.has_focus() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let label = if busy { "Speaking…" } else { "Speak" };
            if widgets::primary_button(ui, ic::MEGAPHONE, label)
                .on_hover_text("Into the mic and your speakers (Ctrl+Enter)")
                .clicked()
                || send
            {
                let t = app.speak.text.clone();
                run(app, Mode::Speak, t);
            }
            if widgets::ghost_button(ui, ic::HEADPHONES, "Preview").on_hover_text("Only you hear it").clicked() {
                let t = app.speak.text.clone();
                run(app, Mode::Preview, t);
            }
            if widgets::ghost_button(ui, ic::PLUS, "Save as pad").clicked() {
                let t = app.speak.text.clone();
                run(app, Mode::Save, t);
            }
            if busy {
                ui.add(egui::Spinner::new().color(accent()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(widgets::faint(format!("{} chars", app.speak.text.chars().count())));
            });
        });
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.label(widgets::faint("Quick:"));
            for q in QUICK {
                if widgets::chip(ui, q, TEXT_DIM)
                    .interact(Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    run(app, Mode::Speak, q.to_string());
                }
            }
        });
    });
}

fn voice_card(app: &mut RiffApp, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        widgets::section_title(ui, ic::ROBOT, "Voice", "Piper sounds natural, espeak is instant and tiny");
        let mut engine = app.settings.tts.engine;
        if widgets::segmented(
            ui,
            &mut engine,
            &[(TtsEngine::Piper, "Piper · natural"), (TtsEngine::Espeak, "espeak · retro")],
        ) {
            app.settings.tts.engine = engine;
            app.settings_dirty = true;
        }
        ui.add_space(10.0);
        let (piper_ok, espeak_ok) = app.speak.bins.unwrap_or((false, false));
        match engine {
            TtsEngine::Piper => piper_settings(app, ui, piper_ok),
            TtsEngine::Espeak => espeak_settings(app, ui, espeak_ok),
        }
    });
}

fn missing_bin(ui: &mut Ui, pkg: &str) {
    egui::Frame::new().fill(alpha(theme::WARN, 22)).corner_radius(10).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(egui::RichText::new(format!("{}  {pkg} isn't installed", ic::WARNING)).color(theme::WARN));
        ui.label(widgets::faint(format!(
            "Riff's flake includes it — run Riff from `nix develop`, or add `{pkg}` to your system packages."
        )));
    });
    ui.add_space(8.0);
}

fn piper_settings(app: &mut RiffApp, ui: &mut Ui, ok: bool) {
    if !ok {
        missing_bin(ui, "piper-tts");
    }
    let installed = app.speak.installed.clone().unwrap_or_default();
    let t = &mut app.settings.tts;
    if installed.is_empty() {
        ui.label(widgets::dim("No voices yet — grab one below (≈ 60 MB each). Until then Riff speaks with espeak."));
    } else {
        ui.horizontal(|ui| {
            ui.label(widgets::dim("Voice"));
            egui::ComboBox::from_id_salt("piper-voice")
                .width(ui.available_width() - 8.0)
                .selected_text(&t.piper_voice)
                .show_ui(ui, |ui| {
                    for k in &installed {
                        if ui.selectable_value(&mut t.piper_voice, k.clone(), k).changed() {
                            app.settings_dirty = true;
                        }
                    }
                });
        });
    }
    if widgets::slider_row(ui, "Speed", &mut t.speed, 0.5..=2.0, |v| format!("{v:.2}×")) {
        app.settings_dirty = true;
    }
    let multi = app
        .speak
        .catalog
        .as_ref()
        .and_then(|c| c.as_ref().ok())
        .and_then(|c| c.iter().find(|v| v.key == app.settings.tts.piper_voice))
        .map(|v| v.num_speakers)
        .unwrap_or(1);
    if multi > 1 {
        let mut sp = app.settings.tts.piper_speaker as f32;
        if widgets::slider_row(ui, "Speaker", &mut sp, 0.0..=(multi - 1) as f32, |v| format!("#{}", v as u32)) {
            app.settings.tts.piper_speaker = sp as u32;
            app.settings_dirty = true;
        }
    }

    ui.add_space(12.0);
    widgets::label_caps(ui, "Get voices");
    if app.speak.catalog.is_none() && app.speak.catalog_job.is_none() {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let core = app.core.clone();
        std::thread::spawn(move || {
            let _ = tx.send(tts::fetch_catalog());
            core.repaint();
        });
        app.speak.catalog_job = Some(rx);
    }
    match &app.speak.catalog {
        None => {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().color(accent()));
                ui.label(widgets::dim("Loading the Piper voice catalog…"));
            });
        }
        Some(Err(e)) => {
            ui.label(egui::RichText::new(format!("Couldn't load voices: {e}")).color(theme::DANGER).size(12.5));
            if ui.button("Retry").clicked() {
                app.speak.catalog = None;
            }
        }
        Some(Ok(list)) => {
            let list = list.clone();
            widgets::search_field(
                ui,
                &mut app.speak.filter,
                "Filter by language or name (e.g. arabic, en_GB)",
                ui.available_width() - 60.0,
                egui::Id::new("voice-filter"),
            );
            ui.add_space(6.0);
            let q = app.speak.filter.to_lowercase();
            let shown: Vec<&PiperVoice> = list
                .iter()
                .filter(|v| {
                    q.is_empty()
                        || v.key.to_lowercase().contains(&q)
                        || v.language.name_english.to_lowercase().contains(&q)
                        || v.language.country_english.to_lowercase().contains(&q)
                })
                .take(if q.is_empty() { tts::FEATURED.len() } else { 40 })
                .collect();
            if app.speak.done.is_none() {
                app.speak.done = Some(crossbeam_channel::unbounded());
            }
            egui::ScrollArea::vertical().id_salt("voices").max_height(300.0).show(ui, |ui| {
                for v in shown {
                    voice_row(app, ui, v);
                }
            });
            if q.is_empty() {
                ui.label(widgets::faint(format!("{} voices available — type to search all of them", list.len())));
            }
        }
    }
}

fn voice_row(app: &mut RiffApp, ui: &mut Ui, v: &PiperVoice) {
    let installed = tts::is_installed(&v.key);
    let current = app.settings.tts.piper_voice == v.key;
    egui::Frame::new()
        .fill(if current { alpha(accent(), 22) } else { theme::SURFACE_LO })
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.set_width(ui.available_width() - 110.0);
                    ui.label(egui::RichText::new(v.label()).color(TEXT).size(13.0));
                    ui.label(widgets::faint(format!(
                        "{} · {:.0} MB{}",
                        v.key,
                        v.size_mb(),
                        if v.num_speakers > 1 { format!(" · {} speakers", v.num_speakers) } else { String::new() }
                    )));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(p) = app.speak.downloads.get(&v.key) {
                        let f = p.load(Ordering::Relaxed) as f32 / 1000.0;
                        ui.add(egui::ProgressBar::new(f).desired_width(90.0).corner_radius(6).fill(accent()));
                    } else if installed {
                        if current {
                            widgets::chip(ui, &format!("{} In use", ic::CHECK), theme::OK);
                        } else if ui.button("Use").clicked() {
                            app.settings.tts.piper_voice = v.key.clone();
                            app.settings_dirty = true;
                        }
                    } else if widgets::ghost_button(ui, ic::DOWNLOAD_SIMPLE, "Get").clicked() {
                        let progress = Arc::new(AtomicU32::new(0));
                        app.speak.downloads.insert(v.key.clone(), progress.clone());
                        let tx = app.speak.done.as_ref().unwrap().0.clone();
                        let voice = v.clone();
                        let core = app.core.clone();
                        std::thread::spawn(move || {
                            let r = tts::download_voice(&voice, &progress);
                            let _ = tx.send((voice.key.clone(), r));
                            core.repaint();
                        });
                    }
                });
            });
        });
    ui.add_space(4.0);
}

fn espeak_settings(app: &mut RiffApp, ui: &mut Ui, ok: bool) {
    if !ok {
        missing_bin(ui, "espeak-ng");
    }
    if app.speak.espeak_voices.is_none() {
        app.speak.espeak_voices = Some(tts::espeak_voices(&app.settings.tts.espeak_bin));
    }
    let voices = app.speak.espeak_voices.clone().unwrap_or_default();
    let t = &mut app.settings.tts;
    let mut dirty = false;
    ui.horizontal(|ui| {
        ui.label(widgets::dim("Language"));
        let cur = voices
            .iter()
            .find(|v| v.id == t.espeak_voice)
            .map(|v| format!("{} ({})", v.name, v.id))
            .unwrap_or(t.espeak_voice.clone());
        egui::ComboBox::from_id_salt("espeak-voice")
            .width(ui.available_width() - 8.0)
            .selected_text(cur)
            .height(320.0)
            .show_ui(ui, |ui| {
                for v in &voices {
                    dirty |= ui
                        .selectable_value(&mut t.espeak_voice, v.id.clone(), format!("{} ({})", v.name, v.id))
                        .changed();
                }
            });
    });
    ui.horizontal(|ui| {
        ui.label(widgets::dim("Character"));
        let cur =
            tts::ESPEAK_VARIANTS.iter().find(|(k, _)| *k == t.espeak_variant).map(|(_, n)| *n).unwrap_or("Default");
        egui::ComboBox::from_id_salt("espeak-variant").width(ui.available_width() - 8.0).selected_text(cur).show_ui(
            ui,
            |ui| {
                for (k, n) in tts::ESPEAK_VARIANTS {
                    dirty |= ui.selectable_value(&mut t.espeak_variant, k.to_string(), *n).changed();
                }
            },
        );
    });
    dirty |= widgets::slider_row(ui, "Speed", &mut t.speed, 0.5..=2.0, |v| format!("{v:.2}×"));
    let mut pitch = t.pitch as f32;
    if widgets::slider_row(ui, "Pitch", &mut pitch, 0.0..=99.0, |v| format!("{v:.0}")) {
        t.pitch = pitch as u32;
        dirty = true;
    }
    if dirty {
        app.settings_dirty = true;
    }
}

fn history(app: &mut RiffApp, ui: &mut Ui) {
    if app.speak.history.is_empty() {
        return;
    }
    widgets::card(ui, |ui| {
        widgets::label_caps(ui, "Recent lines");
        ui.add_space(4.0);
        let items = app.speak.history.clone();
        for (i, l) in items.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
            if resp.hovered() {
                ui.painter().rect_filled(rect, 10.0, SURFACE_HI);
            }
            let mut job = egui::text::LayoutJob::single_section(
                l.text.clone(),
                egui::TextFormat::simple(theme::body(14.0), TEXT),
            );
            job.wrap = egui::text::TextWrapping {
                max_width: rect.width() - 140.0,
                max_rows: 1,
                break_anywhere: true,
                overflow_character: Some('…'),
            };
            let g = ui.fonts_mut(|f| f.layout_job(job));
            ui.painter().galley(rect.left_top() + vec2(10.0, 7.0), g, TEXT);
            ui.painter().text(
                rect.left_bottom() + vec2(10.0, -8.0),
                Align2::LEFT_BOTTOM,
                &l.voice,
                theme::body(11.0),
                TEXT_FAINT,
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(vec2(6.0, 10.0)))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
            );
            if widgets::small_icon_button(&mut child, ic::PLUS, "Save as pad").clicked() {
                save_line(app, l);
            }
            if widgets::small_icon_button(&mut child, ic::HEADPHONES, "Preview").clicked() {
                app.core.play_adhoc("tts", l.clip.clone(), SoundFx { to_mic: false, ..Default::default() });
            }
            if widgets::small_icon_button(&mut child, ic::PLAY, "Say it again").clicked() {
                app.core.play_adhoc("tts", l.clip.clone(), SoundFx::default());
            }
            if i + 1 < items.len() {
                ui.add_space(2.0);
            }
        }
        let _ = TEXT_DIM;
    });
}
