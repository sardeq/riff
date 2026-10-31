use super::theme::{self, TEXT, TEXT_DIM, alpha};
use super::widgets::{self, accent};
use crate::app::{DeviceInfo, RecordTarget, RiffApp};
use crate::model::ClickMode;
use crate::paths::paths;
use egui::{Sense, Stroke, Ui, vec2};
use egui_phosphor::regular as ic;

pub fn show(app: &mut RiffApp, ui: &mut Ui) {
    ui.label(egui::RichText::new("Settings").font(theme::display(28.0)).color(TEXT));
    ui.label(widgets::dim("Everything saves automatically."));
    ui.add_space(14.0);

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.set_max_width(760.0);
        routing(app, ui);
        ui.add_space(14.0);
        playback(app, ui);
        ui.add_space(14.0);
        hotkeys(app, ui);
        ui.add_space(14.0);
        integrations(app, ui);
        ui.add_space(14.0);
        appearance(app, ui);
        ui.add_space(14.0);
        about(app, ui);
        ui.add_space(24.0);
    });
}

#[cfg(target_os = "linux")]
fn load_devices(app: &mut RiffApp) {
    use crate::audio::routing;
    let conv = |v: Vec<routing::Device>| {
        v.into_iter().map(|d| DeviceInfo { name: d.name, description: d.description }).collect()
    };
    app.devices = Some((conv(routing::input_devices()), conv(routing::output_devices())));
}

#[cfg(not(target_os = "linux"))]
fn load_devices(app: &mut RiffApp) {
    app.devices = Some((Vec::new(), Vec::new()));
}

fn device_combo(ui: &mut Ui, id: &str, current: &mut Option<String>, list: &[DeviceInfo]) -> bool {
    let label = current
        .as_ref()
        .map(|n| list.iter().find(|d| &d.name == n).map(|d| d.description.clone()).unwrap_or(n.clone()))
        .unwrap_or_else(|| "System default".into());
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).width(300.0).selected_text(label).show_ui(ui, |ui| {
        changed |= ui.selectable_value(current, None, "System default").changed();
        for d in list {
            changed |= ui.selectable_value(current, Some(d.name.clone()), &d.description).changed();
        }
    });
    changed
}

fn routing(app: &mut RiffApp, ui: &mut Ui) {
    if app.devices.is_none() {
        load_devices(app);
    }
    widgets::card(ui, |ui| {
        widgets::section_title(ui, ic::BROADCAST, "Virtual microphone", "How your sounds reach Discord, games and OBS");
        let st = app.status.clone();
        let (col, text) = if st.virtual_ok {
            (theme::OK, "Live — choose “Riff Microphone” as the input device in your voice app.")
        } else if !app.settings.virtual_mic {
            (TEXT_DIM, "Off — sounds only play on your speakers.")
        } else {
            (theme::WARN, "Couldn't create the virtual device. Is PipeWire (pipewire-pulse) or PulseAudio running?")
        };
        egui::Frame::new().fill(alpha(col, 20)).corner_radius(10).inner_margin(10).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("●").color(col));
                ui.label(egui::RichText::new(text).color(TEXT));
            });
            #[cfg(target_os = "linux")]
            if let Some(e) = &app.vmic_error {
                ui.label(widgets::faint(e));
            }
            if let Some(e) = &st.error {
                ui.label(widgets::faint(e));
            }
        });
        ui.add_space(8.0);

        let mut vm = app.settings.virtual_mic;
        if widgets::toggle_row(
            ui,
            "Create Riff Microphone",
            "A virtual input device (PipeWire/PulseAudio null sink). Removed when Riff quits.",
            &mut vm,
        ) {
            app.settings.virtual_mic = vm;
            app.settings_dirty = true;
            #[cfg(target_os = "linux")]
            app.set_virtual_mic(vm);
            apply_engine(app);
        }
        ui.separator();

        let mut mp = app.settings.mic_passthrough;
        if widgets::toggle_row(
            ui,
            "Mix in my real microphone",
            "So one input carries both your voice and your sounds",
            &mut mp,
        ) {
            app.settings.mic_passthrough = mp;
            app.settings_dirty = true;
            apply_engine(app);
        }
        let (inputs, outputs) = app.devices.clone().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(widgets::dim("Microphone"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::small_icon_button(ui, ic::ARROWS_CLOCKWISE, "Refresh devices").clicked() {
                    app.devices = None;
                }
                if device_combo(ui, "input-dev", &mut app.settings.input_device, &inputs) {
                    app.settings_dirty = true;
                    apply_engine(app);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(widgets::dim("I hear sounds on"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(30.0);
                if device_combo(ui, "output-dev", &mut app.settings.monitor_device, &outputs) {
                    app.settings_dirty = true;
                    apply_engine(app);
                }
            });
        });
        if widgets::slider_row(ui, "Mic level", &mut app.settings.mic_gain, 0.0..=2.0, |v| format!("{:.0}%", v * 100.0))
        {
            app.settings_dirty = true;
            app.mix_dirty = true;
        }
        widgets::meter(ui, app.status.peak_mic_in, ui.available_width());
        ui.add_space(6.0);
        if widgets::toggle_row(
            ui,
            "Duck my mic while sounds play",
            "Lowers your voice so the sound comes through clearly",
            &mut app.settings.duck_mic,
        ) {
            app.settings_dirty = true;
            app.mix_dirty = true;
        }
        if app.settings.duck_mic
            && widgets::slider_row(ui, "Mic level while ducked", &mut app.settings.duck_amount, 0.0..=1.0, |v| {
                format!("{:.0}%", v * 100.0)
            })
        {
            app.settings_dirty = true;
            app.mix_dirty = true;
        }
    });
}

fn apply_engine(app: &mut RiffApp) {
    // write settings through first so the engine config reflects them
    *app.core.settings.write() = app.settings.clone();
    app.core.reconfigure();
}

fn playback(app: &mut RiffApp, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        widgets::section_title(ui, ic::PLAY, "Playback", "");
        ui.horizontal(|ui| {
            ui.label(widgets::dim("Pressing a pad that's already playing"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut m = app.settings.click_mode;
                if widgets::segmented(
                    ui,
                    &mut m,
                    &[(ClickMode::Restart, "Restarts"), (ClickMode::Toggle, "Stops"), (ClickMode::Layer, "Layers")],
                ) {
                    app.settings.click_mode = m;
                    app.settings_dirty = true;
                }
            });
        });
        let s = &mut app.settings;
        let mut d = false;
        d |= widgets::toggle_row(
            ui,
            "One sound at a time",
            "A new sound cuts off whatever is playing",
            &mut s.exclusive,
        );
        d |= widgets::toggle_row(
            ui,
            "Auto-level sounds",
            "Evens out loudness so quiet clips aren't drowned and loud ones don't blast",
            &mut s.auto_level,
        );
        let lim = widgets::toggle_row(
            ui,
            "Limiter",
            "Soft-limits the output so stacked sounds never clip harshly",
            &mut s.limiter,
        );
        d |= lim;
        d |= widgets::toggle_row(
            ui,
            "Copy imported files into Riff",
            "Keeps your library working if you move or delete the originals",
            &mut s.copy_imports,
        );
        if d {
            app.settings_dirty = true;
            app.mix_dirty |= lim;
        }
    });
}

fn hotkey_button(app: &mut RiffApp, ui: &mut Ui, target: RecordTarget, current: Option<String>) {
    let recording = app.recording.as_ref() == Some(&target);
    let label = if recording { "Press keys…".to_string() } else { current.unwrap_or_else(|| "Not set".into()) };
    let (r, resp) = ui.allocate_exact_size(vec2(170.0, 32.0), Sense::click());
    ui.painter().rect(
        r,
        10.0,
        theme::SURFACE_LO,
        Stroke::new(1.0, if recording { accent() } else { theme::BORDER }),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        format!("{}  {label}", ic::KEYBOARD),
        theme::semibold(12.5),
        TEXT,
    );
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        if recording { app.finish_recording(None) } else { app.start_recording(target) }
    }
}

fn hotkeys(app: &mut RiffApp, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        widgets::section_title(ui, ic::KEYBOARD, "Hotkeys", "Trigger sounds without switching windows");
        let mut g = app.settings.global_hotkeys;
        if widgets::toggle_row(
            ui,
            "Global hotkeys",
            "Reads your keyboards directly (evdev), so it works on niri, Hyprland, Sway, KDE, GNOME and X11",
            &mut g,
        ) {
            app.settings.global_hotkeys = g;
            app.settings_dirty = true;
            app.set_global_hotkeys(g);
        }
        if let Some(l) = &app.listener {
            let st = l.status.lock().clone();
            let (col, msg) = if st.keyboards > 0 {
                (
                    theme::OK,
                    format!(
                        "Listening on {} keyboard device{}",
                        st.keyboards,
                        if st.keyboards == 1 { "" } else { "s" }
                    ),
                )
            } else if st.permission_problem {
                (
                    theme::WARN,
                    "No permission to read keyboards. Add yourself to the `input` group, then log out and back in."
                        .to_string(),
                )
            } else {
                (theme::WARN, "Looking for keyboards…".to_string())
            };
            ui.label(egui::RichText::new(format!("●  {msg}")).color(col).size(12.5));
            if st.permission_problem {
                code(ui, "# configuration.nix\nusers.users.<you>.extraGroups = [ \"input\" ];");
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(widgets::dim("Stop everything"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if app.settings.stop_hotkey.is_some() && widgets::small_icon_button(ui, ic::X, "Clear").clicked() {
                    app.settings.stop_hotkey = None;
                    app.settings_dirty = true;
                }
                let cur = app.settings.stop_hotkey.as_ref().map(|h| h.label());
                hotkey_button(app, ui, RecordTarget::StopAll, cur);
            });
        });
        ui.horizontal(|ui| {
            ui.label(widgets::dim("Toggle my mic"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if app.settings.mic_hotkey.is_some() && widgets::small_icon_button(ui, ic::X, "Clear").clicked() {
                    app.settings.mic_hotkey = None;
                    app.settings_dirty = true;
                }
                let cur = app.settings.mic_hotkey.as_ref().map(|h| h.label());
                hotkey_button(app, ui, RecordTarget::Mic, cur);
            });
        });
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Prefer compositor binds? Riff has a CLI:").color(TEXT_DIM).size(12.5));
        code(
            ui,
            "riff play \"Air horn\"     riff toggle <name>     riff stop\nriff random [board]      riff mic on|off|toggle   riff say \"hello\"",
        );
        code(
            ui,
            "// niri config.kdl\nbinds {\n    Mod+Shift+1 { spawn \"riff\" \"play\" \"Air horn\"; }\n    Mod+Escape  { spawn \"riff\" \"stop\"; }\n}",
        );
    });
}

fn code(ui: &mut Ui, text: &str) {
    egui::Frame::new().fill(theme::SURFACE_LO).corner_radius(10).inner_margin(10).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(text).monospace().color(TEXT_DIM));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if widgets::small_icon_button(ui, ic::COPY, "Copy").clicked() {
                    ui.ctx().copy_text(text.to_string());
                }
            });
        });
    });
}

fn integrations(app: &mut RiffApp, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        widgets::section_title(ui, ic::GLOBE, "Freesound", "Search and add sounds from freesound.org");
        ui.horizontal(|ui| {
            ui.label(widgets::dim("API key"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.link("Get a free key").clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(crate::freesound::API_KEY_URL));
                }
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut app.settings.freesound_key).password(true).desired_width(300.0),
                    )
                    .changed()
                {
                    app.settings_dirty = true;
                }
            });
        });
        ui.add_space(10.0);
        widgets::section_title(ui, ic::ROBOT, "Text to speech", "Commands used for local voices");
        ui.horizontal(|ui| {
            ui.label(widgets::dim("Piper command"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::TextEdit::singleline(&mut app.settings.tts.piper_bin).desired_width(300.0)).changed() {
                    app.settings_dirty = true;
                    app.speak = Default::default();
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(widgets::dim("espeak command"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::TextEdit::singleline(&mut app.settings.tts.espeak_bin).desired_width(300.0)).changed() {
                    app.settings_dirty = true;
                    app.speak = Default::default();
                }
            });
        });
        ui.label(widgets::faint(format!("Voices are stored in {}", paths().voices_dir().display())));
    });
}

fn appearance(app: &mut RiffApp, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        widgets::section_title(ui, ic::PALETTE, "Appearance", "");
        ui.horizontal(|ui| {
            ui.label(widgets::dim("Accent"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for (i, (c, name)) in theme::PALETTE.iter().enumerate().rev() {
                    let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                    ui.painter().circle_filled(r.center(), 10.0, *c);
                    if app.settings.accent == i {
                        ui.painter().circle_stroke(r.center(), 12.0, Stroke::new(2.0, TEXT));
                    }
                    if resp.on_hover_text(*name).clicked() {
                        app.settings.accent = i;
                        app.settings_dirty = true;
                        widgets::set_accent(i);
                        theme::apply(ui.ctx(), *c);
                    }
                }
            });
        });
        if widgets::slider_row(ui, "Pad size", &mut app.settings.pad_size, 0.75..=1.5, |v| format!("{:.0}%", v * 100.0))
        {
            app.settings_dirty = true;
        }
    });
}

fn about(app: &mut RiffApp, ui: &mut Ui) {
    widgets::card(ui, |ui| {
        widgets::section_title(
            ui,
            ic::INFO,
            "About Riff",
            concat!("Version ", env!("CARGO_PKG_VERSION"), " · the successor to BoomBx"),
        );
        ui.horizontal(|ui| {
            if widgets::ghost_button(ui, ic::FOLDER_OPEN, "Open data folder").clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(format!("file://{}", paths().data.display())));
            }
            if widgets::ghost_button(ui, ic::ARROW_SQUARE_OUT, "GitHub").clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab("https://github.com/sardeq/riff"));
            }
        });
        ui.label(widgets::faint(format!("Library: {}", paths().library_file().display())));
        let _ = app;
    });
}
