//! Riff — a modern soundboard with a virtual microphone, global hotkeys,
//! local text-to-speech and Freesound search.

mod app;
mod audio;
mod core;
mod freesound;
mod hotkeys;
mod ipc;
mod model;
mod net;
mod paths;
mod tts;
mod ui;

const HELP: &str = "\
Riff — soundboard with a virtual microphone

USAGE
    riff                      open Riff (or bring the running window to front)
    riff play <sound>         play a sound by name (fuzzy) or id
    riff toggle <sound>       play, or stop if it's already playing
    riff stop [<sound>]       stop one sound, or everything
    riff random [<board>]     play a random sound
    riff pause                pause / resume everything
    riff mic [on|off|toggle]  mix your real mic into Riff Microphone
    riff say <text>           speak text into the mic with your TTS voice
    riff list                 list boards and sounds

Bind these in your compositor, e.g. niri:
    Mod+Shift+1 { spawn \"riff\" \"play\" \"Air horn\"; }
";

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("riff=info,warn")).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(first) = args.first() {
        match first.as_str() {
            "-h" | "--help" | "help" => {
                print!("{HELP}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("riff {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => {
                let line = args.join(" ");
                match ipc::send(&line) {
                    Ok(reply) => {
                        let reply = reply.trim_end();
                        if let Some(err) = reply.strip_prefix("err ") {
                            eprintln!("riff: {err}");
                            std::process::exit(1);
                        }
                        let body = reply.strip_prefix("ok").unwrap_or(reply).trim_start_matches([' ', '\n']);
                        if !body.is_empty() {
                            println!("{body}");
                        }
                        return Ok(());
                    }
                    Err(_) => {
                        eprintln!("riff: Riff isn't running — start it with `riff` first.");
                        std::process::exit(2);
                    }
                }
            }
        }
    }

    // single instance: if Riff is already running, just raise it
    if ipc::send("show").is_ok() {
        println!("Riff is already running.");
        return Ok(());
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Riff")
            .with_app_id("riff")
            .with_inner_size([1240.0, 780.0])
            .with_min_inner_size([820.0, 560.0])
            .with_icon(ui::theme::app_icon()),
        ..Default::default()
    };
    eframe::run_native("Riff", options, Box::new(|cc| Ok(Box::new(app::RiffApp::new(cc)))))
}
