//! Top-level application state and frame loop.

use crate::audio::EngineStatus;
use crate::core::{Core, CoreEvent, ToastKind};
use crate::hotkeys::{self, GlobalListener, Hotkey};
use crate::model::{self, Board, Library, Settings, Sound, SoundSource, new_id};
use crate::paths::paths;
use crate::ui::{discover::DiscoverState, palette::PaletteState, speak::SpeakState, theme, toasts::Toasts, widgets};
use crossbeam_channel::Receiver;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Sounds,
    Speak,
    Discover,
    Settings,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BoardSel {
    All,
    Favorites,
    Recent,
    Board(String),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RecordTarget {
    Sound(String),
    StopAll,
    Mic,
}

pub struct RiffApp {
    pub core: Core,
    events: Receiver<CoreEvent>,
    #[cfg(target_os = "linux")]
    pub vmic: Option<crate::audio::routing::VirtualMic>,
    #[cfg(target_os = "linux")]
    pub vmic_error: Option<String>,
    pub listener: Option<GlobalListener>,
    pub view: View,
    pub board: BoardSel,
    pub search: String,
    pub selected: Option<String>,
    pub renaming_board: Option<(String, String)>,
    pub recording: Option<RecordTarget>,
    pub palette: PaletteState,
    pub toasts: Toasts,
    pub speak: SpeakState,
    pub discover: DiscoverState,
    pub status: EngineStatus,
    /// Snapshot of the library for this frame (never hold the lock while drawing)
    pub lib: Library,
    pub settings: Settings,
    pub settings_dirty: bool,
    pub mix_dirty: bool,
    pub devices: Option<(Vec<DeviceInfo>, Vec<DeviceInfo>)>,
    pub focus_search: bool,
    last_save: Instant,
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub name: String,
    pub description: String,
}

impl RiffApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        let mut settings = Settings::load();
        let mut lib = Library::load();
        if !settings.seeded {
            seed_starters(&mut lib);
            settings.seeded = true;
            settings.save();
            lib.save();
        }
        widgets::set_accent(settings.accent);
        theme::apply(&cc.egui_ctx, theme::color(settings.accent));

        #[cfg(target_os = "linux")]
        let (vmic, vmic_error) = if settings.virtual_mic {
            match crate::audio::routing::VirtualMic::create() {
                Ok(v) => (Some(v), None),
                Err(e) => {
                    log::warn!("virtual mic: {e}");
                    (None, Some(e.to_string()))
                }
            }
        } else {
            (None, None)
        };

        let cfg = crate::core::engine_config(&settings);
        let (core, events) = Core::new(lib.clone(), settings.clone(), cfg);
        core.set_ctx(cc.egui_ctx.clone());
        crate::ipc::serve(core.clone());

        // warm the clip cache so waveforms appear and the first press is instant
        for s in &lib.sounds {
            core.ensure_loaded(&s.id);
        }

        let board = lib
            .board(&settings.last_board)
            .map(|b| BoardSel::Board(b.id.clone()))
            .unwrap_or_else(|| BoardSel::Board(lib.boards[0].id.clone()));

        let mut app = Self {
            core,
            events,
            #[cfg(target_os = "linux")]
            vmic,
            #[cfg(target_os = "linux")]
            vmic_error,
            listener: None,
            view: View::Sounds,
            board,
            search: String::new(),
            selected: None,
            renaming_board: None,
            recording: None,
            palette: PaletteState::default(),
            toasts: Toasts::default(),
            speak: SpeakState::default(),
            discover: DiscoverState::default(),
            status: EngineStatus::default(),
            lib,
            settings,
            settings_dirty: false,
            mix_dirty: false,
            devices: None,
            focus_search: false,
            last_save: Instant::now(),
        };
        app.set_global_hotkeys(app.settings.global_hotkeys);
        app
    }

    pub fn set_global_hotkeys(&mut self, on: bool) {
        if !on {
            self.listener = None;
            return;
        }
        if self.listener.is_some() {
            return;
        }
        let (tx, rx) = crossbeam_channel::unbounded::<Hotkey>();
        let core = self.core.clone();
        std::thread::Builder::new()
            .name("riff-hotkey-dispatch".into())
            .spawn(move || {
                for hk in rx {
                    core.handle_hotkey(hk);
                }
            })
            .expect("spawn dispatcher");
        self.listener = Some(hotkeys::start(tx));
    }

    pub fn global_hotkeys_active(&self) -> bool {
        self.listener.as_ref().is_some_and(|l| {
            let s = l.status.lock();
            s.running && s.keyboards > 0
        })
    }

    #[cfg(target_os = "linux")]
    pub fn set_virtual_mic(&mut self, on: bool) {
        if on && self.vmic.is_none() {
            match crate::audio::routing::VirtualMic::create() {
                Ok(v) => {
                    self.vmic = Some(v);
                    self.vmic_error = None;
                }
                Err(e) => {
                    self.vmic_error = Some(e.to_string());
                    self.toast(ToastKind::Error, format!("Couldn't create virtual mic: {e}"));
                }
            }
        } else if !on {
            self.vmic = None;
        }
    }

    pub fn toast(&mut self, kind: ToastKind, msg: impl Into<String>) {
        self.toasts.push(kind, msg.into());
    }

    // ---------- library helpers (write-through to the shared core) ----------

    pub fn edit_lib(&mut self, f: impl FnOnce(&mut Library)) {
        {
            let mut lib = self.core.lib.write();
            f(&mut lib);
            self.lib = lib.clone();
        }
        self.core.dirty.store(true, Ordering::Relaxed);
    }

    pub fn edit_sound(&mut self, id: &str, f: impl FnOnce(&mut Sound)) {
        let id = id.to_string();
        self.edit_lib(|lib| {
            if let Some(s) = lib.sound_mut(&id) {
                f(s);
            }
        });
    }

    pub fn current_board_id(&self) -> String {
        match &self.board {
            BoardSel::Board(id) if self.lib.board(id).is_some() => id.clone(),
            _ => self.lib.boards[0].id.clone(),
        }
    }

    pub fn visible_sounds(&self) -> Vec<Sound> {
        let q = self.search.trim().to_lowercase();
        let mut v: Vec<Sound> = self
            .lib
            .sounds
            .iter()
            .filter(|s| match &self.board {
                BoardSel::All => true,
                BoardSel::Favorites => s.favorite,
                BoardSel::Recent => s.last_played > 0,
                BoardSel::Board(b) => &s.board == b,
            })
            .filter(|s| q.is_empty() || s.name.to_lowercase().contains(&q))
            .cloned()
            .collect();
        if self.board == BoardSel::Recent {
            v.sort_by_key(|s| std::cmp::Reverse(s.last_played));
            v.truncate(30);
        }
        v
    }

    pub fn import_paths(&mut self, paths_in: Vec<PathBuf>) {
        let mut files = Vec::new();
        for p in paths_in {
            if p.is_dir() {
                collect_audio(&p, &mut files, 0);
            } else if model::is_audio_file(&p) {
                files.push(p);
            }
        }
        if files.is_empty() {
            self.toast(ToastKind::Error, "No audio files found (mp3, wav, ogg, flac, m4a…)");
            return;
        }
        let board = self.current_board_id();
        let copy = self.settings.copy_imports;
        let mut added = Vec::new();
        let mut failed = 0;
        for f in files {
            match model::adopt_file(&f, copy) {
                Ok(path) => {
                    let name = model::pretty_name(&f);
                    let s = Sound {
                        emoji: model::guess_emoji(&name).into(),
                        color: fastrand::usize(..theme::PALETTE.len()),
                        name,
                        path,
                        board: board.clone(),
                        ..Default::default()
                    };
                    added.push(s);
                }
                Err(e) => {
                    log::warn!("import {}: {e}", f.display());
                    failed += 1;
                }
            }
        }
        let n = added.len();
        let ids: Vec<String> = added.iter().map(|s| s.id.clone()).collect();
        self.edit_lib(|lib| lib.sounds.extend(added));
        for id in &ids {
            self.core.ensure_loaded(id);
        }
        if self.board != BoardSel::Board(board.clone()) {
            self.board = BoardSel::Board(board);
        }
        let msg = if failed > 0 {
            format!("Added {n} sounds ({failed} failed)")
        } else {
            format!("Added {n} sound{}", if n == 1 { "" } else { "s" })
        };
        self.toast(ToastKind::Success, msg);
    }

    pub fn import_dialog(&mut self) {
        let files =
            rfd::FileDialog::new().set_title("Add sounds to Riff").add_filter("Audio", model::AUDIO_EXTS).pick_files();
        if let Some(f) = files {
            self.import_paths(f);
        }
    }

    pub fn new_board(&mut self) {
        let n = self.lib.boards.len();
        let b = Board { id: new_id(), name: format!("Board {}", n + 1), color: n % theme::PALETTE.len() };
        let id = b.id.clone();
        self.edit_lib(|lib| lib.boards.push(b));
        self.board = BoardSel::Board(id.clone());
        self.view = View::Sounds;
        self.renaming_board = Some((id, String::new()));
    }

    pub fn duplicate_sound(&mut self, id: &str) {
        let Some(src) = self.lib.sound(id).cloned() else { return };
        let mut copy = src.clone();
        copy.id = new_id();
        copy.name = format!("{} copy", src.name);
        copy.hotkey = None;
        copy.plays = 0;
        if copy.path.starts_with(paths().data.as_path()) {
            // own a separate file so deleting one doesn't break the other
            if let Ok(p) = model::adopt_file(&src.path, true) {
                copy.path = p;
            }
        }
        let new = copy.id.clone();
        self.edit_lib(|lib| {
            let pos = lib.sounds.iter().position(|s| s.id == src.id).map(|p| p + 1).unwrap_or(lib.sounds.len());
            lib.sounds.insert(pos, copy);
        });
        self.core.ensure_loaded(&new);
        self.selected = Some(new);
    }

    pub fn delete_sound(&mut self, id: &str) {
        self.core.stop(id);
        self.core.invalidate(id);
        let name = self.lib.sound(id).map(|s| s.name.clone()).unwrap_or_default();
        let id = id.to_string();
        self.edit_lib(|lib| lib.remove_sound(&id));
        if self.selected.as_deref() == Some(&id) {
            self.selected = None;
        }
        self.toast(ToastKind::Info, format!("Deleted “{name}”"));
    }

    pub fn add_sound(&mut self, s: Sound) {
        let id = s.id.clone();
        self.edit_lib(|lib| lib.sounds.push(s));
        self.core.ensure_loaded(&id);
    }

    pub fn start_recording(&mut self, t: RecordTarget) {
        self.recording = Some(t);
        self.core.recording.store(true, Ordering::Relaxed);
    }

    pub fn finish_recording(&mut self, hk: Option<Hotkey>) {
        let Some(t) = self.recording.take() else { return };
        self.core.recording.store(false, Ordering::Relaxed);
        let Some(hk) = hk else { return };
        // a hotkey belongs to exactly one thing
        let dupe = self.lib.sounds.iter().find(|s| s.hotkey.as_ref() == Some(&hk)).map(|s| s.id.clone());
        if let Some(d) = dupe
            && t != RecordTarget::Sound(d.clone())
        {
            self.edit_sound(&d, |s| s.hotkey = None);
        }
        let plain = !hk.ctrl && !hk.alt && !hk.logo && !hk.shift && hk.key.len() == 1;
        match t {
            RecordTarget::Sound(id) => self.edit_sound(&id, |s| s.hotkey = Some(hk.clone())),
            RecordTarget::StopAll => {
                self.settings.stop_hotkey = Some(hk.clone());
                self.settings_dirty = true;
            }
            RecordTarget::Mic => {
                self.settings.mic_hotkey = Some(hk.clone());
                self.settings_dirty = true;
            }
        }
        if plain && self.global_hotkeys_active() {
            self.toast(
                ToastKind::Info,
                format!(
                    "Heads up: “{}” alone will fire while you type. Numpad, F13–F24 or a modifier work best.",
                    hk.label()
                ),
            );
        }
    }

    fn handle_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.events.try_recv() {
            match ev {
                CoreEvent::Toast(k, m) => self.toast(k, m),
                CoreEvent::HotkeyRecorded(hk) => {
                    if self.recording.is_some() {
                        if hk.key == "Escape" { self.finish_recording(None) } else { self.finish_recording(Some(hk)) }
                    }
                }
                CoreEvent::ShowWindow => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
        }
    }

    /// In-window keyboard handling: shortcuts, hotkey recording, and sound hotkeys
    /// when the global listener isn't available.
    fn handle_keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.egui_wants_keyboard_input();
        let global = self.global_hotkeys_active();
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            let egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } = ev else { continue };
            if self.recording.is_some() {
                if !global {
                    if key == egui::Key::Escape {
                        self.finish_recording(None);
                    } else {
                        self.finish_recording(Hotkey::from_egui(key, modifiers));
                    }
                }
                ctx.input_mut(|i| i.consume_key(modifiers, key));
                continue;
            }
            if self.palette.open {
                continue;
            }
            if modifiers.command && (key == egui::Key::K || key == egui::Key::P) {
                self.palette.toggle();
                continue;
            }
            if modifiers.command && key == egui::Key::F {
                self.view = View::Sounds;
                self.focus_search = true;
                continue;
            }
            if modifiers.command && key == egui::Key::O {
                self.import_dialog();
                continue;
            }
            if typing {
                continue;
            }
            if key == egui::Key::Escape && modifiers.is_none() {
                self.core.stop_all();
                continue;
            }
            if !global && let Some(hk) = Hotkey::from_egui(key, modifiers) {
                self.core.handle_hotkey(hk);
            }
        }
    }

    fn sync_settings(&mut self) {
        if self.settings_dirty {
            self.settings_dirty = false;
            {
                let mut s = self.core.settings.write();
                // keep values other threads own (mic toggle from hotkey/IPC) unless we changed them
                *s = self.settings.clone();
            }
            self.settings.save();
            if self.mix_dirty {
                self.core.push_mix();
                self.mix_dirty = false;
            }
            widgets::set_accent(self.settings.accent);
        }
    }

    fn autosave(&mut self) {
        if self.last_save.elapsed() > Duration::from_secs(2) && self.core.dirty.swap(false, Ordering::Relaxed) {
            self.core.lib.read().save();
            self.last_save = Instant::now();
        }
    }
}

fn collect_audio(dir: &std::path::Path, out: &mut Vec<PathBuf>, depth: u32) {
    if depth > 4 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect_audio(&p, out, depth + 1);
        } else if model::is_audio_file(&p) {
            out.push(p);
        }
    }
}

fn seed_starters(lib: &mut Library) {
    use crate::audio::synth::STARTERS;
    let board = lib.boards[0].id.clone();
    for st in STARTERS {
        let path = paths().sounds_dir().join(st.file);
        if !path.exists() {
            let data = (st.render)();
            if let Err(e) = crate::audio::clip::write_wav(&path, &data, crate::audio::OUT_RATE) {
                log::warn!("write starter {}: {e}", st.name);
                continue;
            }
        }
        lib.sounds.push(Sound {
            name: st.name.into(),
            emoji: st.emoji.into(),
            color: st.color,
            path,
            board: board.clone(),
            source: SoundSource::Builtin,
            ..Default::default()
        });
    }
}

impl eframe::App for RiffApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_events(ctx);
        self.autosave();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // fresh snapshots for this frame
        self.lib = self.core.lib.read().clone();
        if !self.settings_dirty {
            self.settings = self.core.settings.read().clone();
        }
        self.status = self.core.engine.status();

        self.handle_keys(&ctx);

        // drag & drop import
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect()
        });
        if !dropped.is_empty() {
            self.view = View::Sounds;
            self.import_paths(dropped);
        }

        crate::ui::draw(self, ui);

        self.sync_settings();
        if !self.status.voices.is_empty() || self.toasts.animating() || self.speak.busy() || self.discover.busy() {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else {
            // keep meters and the mic level alive at a low rate
            ctx.request_repaint_after(Duration::from_millis(if self.settings.mic_passthrough { 50 } else { 250 }));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.core.lib.read().save();
        self.settings.save();
        self.core.engine.send(crate::audio::Cmd::Shutdown);
        crate::ipc::cleanup();
        std::thread::sleep(Duration::from_millis(60));
        #[cfg(target_os = "linux")]
        {
            self.vmic = None; // unloads the PipeWire modules
        }
    }
}
