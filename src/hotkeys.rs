//! Hotkeys: a serialisable key combo, egui (in-window) matching, and a global
//! listener that reads keyboards directly via evdev — this works on every
//! Wayland compositor (niri, Hyprland, Sway, KDE, GNOME) and on X11.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub struct Hotkey {
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub logo: bool,
    /// Canonical key name, e.g. "A", "5", "F13", "KP7", "Space"
    pub key: String,
}

impl Hotkey {
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.logo {
            parts.push("Super");
        }
        let key = pretty_key(&self.key);
        parts.push(&key);
        parts.join("+")
    }

    /// Short label for pad badges ("⌃⇧F1" style, but readable).
    pub fn short(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push('^');
        }
        if self.alt {
            s.push('⌥');
        }
        if self.shift {
            s.push('⇧');
        }
        if self.logo {
            s.push('◆');
        }
        s + &pretty_key(&self.key)
    }

    fn xkb_key(&self) -> String {
        let k = self.key.as_str();
        if let Some(n) = k.strip_prefix("KP") {
            return match n {
                "Plus" => "KP_Add".into(),
                "Minus" => "KP_Subtract".into(),
                "Asterisk" => "KP_Multiply".into(),
                "Slash" => "KP_Divide".into(),
                "Enter" => "KP_Enter".into(),
                "Dot" => "KP_Decimal".into(),
                d => format!("KP_{d}"),
            };
        }
        match k {
            "Space" => "space".into(),
            "Enter" => "Return".into(),
            "Escape" => "Escape".into(),
            "Minus" => "minus".into(),
            "Equals" => "equal".into(),
            "Comma" => "comma".into(),
            "Period" => "period".into(),
            "Slash" => "slash".into(),
            "Backslash" => "backslash".into(),
            "Semicolon" => "semicolon".into(),
            "Quote" => "apostrophe".into(),
            "Backtick" => "grave".into(),
            "OpenBracket" => "bracketleft".into(),
            "CloseBracket" => "bracketright".into(),
            "PageUp" => "Prior".into(),
            "PageDown" => "Next".into(),
            other => other.into(),
        }
    }

    /// niri config line: `Ctrl+Shift+F1 { spawn "riff" "play" "Air horn"; }`
    pub fn niri_bind(&self, cmd: &[&str]) -> String {
        let mut mods = String::new();
        if self.logo {
            mods.push_str("Super+");
        }
        if self.ctrl {
            mods.push_str("Ctrl+");
        }
        if self.alt {
            mods.push_str("Alt+");
        }
        if self.shift {
            mods.push_str("Shift+");
        }
        let args: Vec<String> = std::iter::once("riff").chain(cmd.iter().copied()).map(|a| format!("{a:?}")).collect();
        format!("{mods}{} {{ spawn {}; }}", self.xkb_key(), args.join(" "))
    }

    /// Hyprland config line: `bind = CTRL SHIFT, F1, exec, riff play "Air horn"`
    pub fn hyprland_bind(&self, cmd: &[&str]) -> String {
        let mut mods = Vec::new();
        if self.logo {
            mods.push("SUPER");
        }
        if self.ctrl {
            mods.push("CTRL");
        }
        if self.alt {
            mods.push("ALT");
        }
        if self.shift {
            mods.push("SHIFT");
        }
        let args: Vec<String> =
            cmd.iter().map(|a| if a.contains(' ') { format!("{a:?}") } else { a.to_string() }).collect();
        format!("bind = {}, {}, exec, riff {}", mods.join(" "), self.xkb_key(), args.join(" "))
    }

    pub fn from_egui(key: egui::Key, m: egui::Modifiers) -> Option<Self> {
        if matches!(key, egui::Key::Escape) {
            return None;
        }
        Some(Self { ctrl: m.ctrl, shift: m.shift, alt: m.alt, logo: m.mac_cmd, key: key.name().to_string() })
    }
}

fn pretty_key(k: &str) -> String {
    match k {
        "OpenBracket" => "[".into(),
        "CloseBracket" => "]".into(),
        "Minus" => "-".into(),
        "Equals" => "=".into(),
        "Comma" => ",".into(),
        "Period" => ".".into(),
        "Slash" => "/".into(),
        "Backslash" => "\\".into(),
        "Semicolon" => ";".into(),
        "Quote" => "'".into(),
        "Backtick" => "`".into(),
        "Space" => "Space".into(),
        k if k.starts_with("KP") => format!("Num{}", &k[2..]),
        k => k.into(),
    }
}

#[cfg(target_os = "linux")]
pub use global::*;

#[cfg(target_os = "linux")]
mod global {
    use super::Hotkey;
    use crossbeam_channel::Sender;
    use evdev::{EventSummary, KeyCode};
    use parking_lot::Mutex;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::time::Duration;

    #[derive(Clone, Debug, Default)]
    pub struct ListenerStatus {
        pub running: bool,
        pub keyboards: usize,
        pub permission_problem: bool,
    }

    /// Reads all keyboards in the background; dropping it stops the listener.
    pub struct GlobalListener {
        pub status: Arc<Mutex<ListenerStatus>>,
        stop: Arc<AtomicBool>,
    }

    impl Drop for GlobalListener {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    const CTRL: u8 = 1;
    const SHIFT: u8 = 2;
    const ALT: u8 = 4;
    const LOGO: u8 = 8;

    fn modifier_bit(k: KeyCode) -> Option<u8> {
        Some(match k {
            KeyCode::KEY_LEFTCTRL | KeyCode::KEY_RIGHTCTRL => CTRL,
            KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT => SHIFT,
            KeyCode::KEY_LEFTALT | KeyCode::KEY_RIGHTALT => ALT,
            KeyCode::KEY_LEFTMETA | KeyCode::KEY_RIGHTMETA => LOGO,
            _ => return None,
        })
    }

    pub fn key_name(k: KeyCode) -> Option<&'static str> {
        use KeyCode as K;
        const TABLE: &[(KeyCode, &str)] = &[
            (K::KEY_A, "A"),
            (K::KEY_B, "B"),
            (K::KEY_C, "C"),
            (K::KEY_D, "D"),
            (K::KEY_E, "E"),
            (K::KEY_F, "F"),
            (K::KEY_G, "G"),
            (K::KEY_H, "H"),
            (K::KEY_I, "I"),
            (K::KEY_J, "J"),
            (K::KEY_K, "K"),
            (K::KEY_L, "L"),
            (K::KEY_M, "M"),
            (K::KEY_N, "N"),
            (K::KEY_O, "O"),
            (K::KEY_P, "P"),
            (K::KEY_Q, "Q"),
            (K::KEY_R, "R"),
            (K::KEY_S, "S"),
            (K::KEY_T, "T"),
            (K::KEY_U, "U"),
            (K::KEY_V, "V"),
            (K::KEY_W, "W"),
            (K::KEY_X, "X"),
            (K::KEY_Y, "Y"),
            (K::KEY_Z, "Z"),
            (K::KEY_0, "0"),
            (K::KEY_1, "1"),
            (K::KEY_2, "2"),
            (K::KEY_3, "3"),
            (K::KEY_4, "4"),
            (K::KEY_5, "5"),
            (K::KEY_6, "6"),
            (K::KEY_7, "7"),
            (K::KEY_8, "8"),
            (K::KEY_9, "9"),
            (K::KEY_F1, "F1"),
            (K::KEY_F2, "F2"),
            (K::KEY_F3, "F3"),
            (K::KEY_F4, "F4"),
            (K::KEY_F5, "F5"),
            (K::KEY_F6, "F6"),
            (K::KEY_F7, "F7"),
            (K::KEY_F8, "F8"),
            (K::KEY_F9, "F9"),
            (K::KEY_F10, "F10"),
            (K::KEY_F11, "F11"),
            (K::KEY_F12, "F12"),
            (K::KEY_F13, "F13"),
            (K::KEY_F14, "F14"),
            (K::KEY_F15, "F15"),
            (K::KEY_F16, "F16"),
            (K::KEY_F17, "F17"),
            (K::KEY_F18, "F18"),
            (K::KEY_F19, "F19"),
            (K::KEY_F20, "F20"),
            (K::KEY_F21, "F21"),
            (K::KEY_F22, "F22"),
            (K::KEY_F23, "F23"),
            (K::KEY_F24, "F24"),
            (K::KEY_KP0, "KP0"),
            (K::KEY_KP1, "KP1"),
            (K::KEY_KP2, "KP2"),
            (K::KEY_KP3, "KP3"),
            (K::KEY_KP4, "KP4"),
            (K::KEY_KP5, "KP5"),
            (K::KEY_KP6, "KP6"),
            (K::KEY_KP7, "KP7"),
            (K::KEY_KP8, "KP8"),
            (K::KEY_KP9, "KP9"),
            (K::KEY_KPPLUS, "KPPlus"),
            (K::KEY_KPMINUS, "KPMinus"),
            (K::KEY_KPASTERISK, "KPAsterisk"),
            (K::KEY_KPSLASH, "KPSlash"),
            (K::KEY_KPENTER, "KPEnter"),
            (K::KEY_KPDOT, "KPDot"),
            (K::KEY_SPACE, "Space"),
            (K::KEY_ENTER, "Enter"),
            (K::KEY_TAB, "Tab"),
            (K::KEY_BACKSPACE, "Backspace"),
            (K::KEY_ESC, "Escape"),
            (K::KEY_INSERT, "Insert"),
            (K::KEY_DELETE, "Delete"),
            (K::KEY_HOME, "Home"),
            (K::KEY_END, "End"),
            (K::KEY_PAGEUP, "PageUp"),
            (K::KEY_PAGEDOWN, "PageDown"),
            (K::KEY_UP, "Up"),
            (K::KEY_DOWN, "Down"),
            (K::KEY_LEFT, "Left"),
            (K::KEY_RIGHT, "Right"),
            (K::KEY_MINUS, "Minus"),
            (K::KEY_EQUAL, "Equals"),
            (K::KEY_LEFTBRACE, "OpenBracket"),
            (K::KEY_RIGHTBRACE, "CloseBracket"),
            (K::KEY_SEMICOLON, "Semicolon"),
            (K::KEY_APOSTROPHE, "Quote"),
            (K::KEY_GRAVE, "Backtick"),
            (K::KEY_BACKSLASH, "Backslash"),
            (K::KEY_COMMA, "Comma"),
            (K::KEY_DOT, "Period"),
            (K::KEY_SLASH, "Slash"),
            (K::KEY_PAUSE, "Pause"),
            (K::KEY_SCROLLLOCK, "ScrollLock"),
            (K::KEY_SYSRQ, "PrintScreen"),
        ];
        TABLE.iter().find(|(c, _)| *c == k).map(|(_, n)| *n)
    }

    fn is_keyboard(d: &evdev::Device) -> bool {
        d.supported_keys()
            .is_some_and(|k| k.contains(KeyCode::KEY_A) || k.contains(KeyCode::KEY_KP0) || k.contains(KeyCode::KEY_F13))
    }

    pub fn start(tx: Sender<Hotkey>) -> GlobalListener {
        let status = Arc::new(Mutex::new(ListenerStatus { running: true, ..Default::default() }));
        let stop = Arc::new(AtomicBool::new(false));
        let mods = Arc::new(AtomicU8::new(0));
        let open: Arc<Mutex<HashSet<PathBuf>>> = Arc::default();

        let (st, sp) = (status.clone(), stop.clone());
        std::thread::Builder::new()
            .name("riff-hotkeys".into())
            .spawn(move || {
                while !sp.load(Ordering::Relaxed) {
                    let mut found_any = false;
                    for (path, dev) in evdev::enumerate() {
                        found_any = true;
                        if !is_keyboard(&dev) || open.lock().contains(&path) {
                            continue;
                        }
                        open.lock().insert(path.clone());
                        spawn_reader(path, dev, tx.clone(), mods.clone(), sp.clone(), open.clone());
                    }
                    let n = open.lock().len();
                    {
                        let mut s = st.lock();
                        s.keyboards = n;
                        // devices exist but none could be opened → not in the `input` group
                        s.permission_problem = n == 0 && !found_any && input_nodes_exist();
                    }
                    std::thread::sleep(Duration::from_secs(4));
                }
                st.lock().running = false;
            })
            .expect("spawn hotkey thread");
        GlobalListener { status, stop }
    }

    fn input_nodes_exist() -> bool {
        std::fs::read_dir("/dev/input")
            .map(|rd| rd.filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().starts_with("event")))
            .unwrap_or(false)
    }

    fn spawn_reader(
        path: PathBuf,
        mut dev: evdev::Device,
        tx: Sender<Hotkey>,
        mods: Arc<AtomicU8>,
        stop: Arc<AtomicBool>,
        open: Arc<Mutex<HashSet<PathBuf>>>,
    ) {
        let _ = std::thread::Builder::new().name("riff-kbd".into()).spawn(move || {
            'read: while !stop.load(Ordering::Relaxed) {
                let events = match dev.fetch_events() {
                    Ok(e) => e,
                    Err(_) => break, // unplugged
                };
                for ev in events {
                    if stop.load(Ordering::Relaxed) {
                        break 'read;
                    }
                    if let EventSummary::Key(_, code, value) = ev.destructure() {
                        if let Some(bit) = modifier_bit(code) {
                            match value {
                                1 => mods.fetch_or(bit, Ordering::Relaxed),
                                0 => mods.fetch_and(!bit, Ordering::Relaxed),
                                _ => 0,
                            };
                            continue;
                        }
                        if value != 1 {
                            continue; // ignore releases and auto-repeat
                        }
                        if let Some(name) = key_name(code) {
                            let m = mods.load(Ordering::Relaxed);
                            let hk = Hotkey {
                                ctrl: m & CTRL != 0,
                                shift: m & SHIFT != 0,
                                alt: m & ALT != 0,
                                logo: m & LOGO != 0,
                                key: name.to_string(),
                            };
                            if tx.send(hk).is_err() {
                                break 'read;
                            }
                        }
                    }
                }
            }
            open.lock().remove(&path);
        });
    }
}

/// Non-Linux: global hotkeys are not implemented yet; in-window hotkeys still work.
#[cfg(not(target_os = "linux"))]
mod global_stub {
    #[derive(Clone, Debug, Default)]
    pub struct ListenerStatus {
        pub running: bool,
        pub keyboards: usize,
        pub permission_problem: bool,
    }
    pub struct GlobalListener {
        pub status: std::sync::Arc<parking_lot::Mutex<ListenerStatus>>,
    }
    pub fn start(_tx: crossbeam_channel::Sender<super::Hotkey>) -> GlobalListener {
        GlobalListener { status: Default::default() }
    }
}
#[cfg(not(target_os = "linux"))]
pub use global_stub::*;

#[cfg(test)]
mod tests {
    use super::Hotkey;

    #[test]
    fn labels_and_binds() {
        let hk = Hotkey { ctrl: true, shift: true, key: "KP7".into(), ..Default::default() };
        assert_eq!(hk.label(), "Ctrl+Shift+Num7");
        assert_eq!(hk.niri_bind(&["play", "Air horn"]), r#"Ctrl+Shift+KP_7 { spawn "riff" "play" "Air horn"; }"#);
        assert_eq!(hk.hyprland_bind(&["play", "Air horn"]), r#"bind = CTRL SHIFT, KP_7, exec, riff play "Air horn""#);
        let f = Hotkey { logo: true, key: "F13".into(), ..Default::default() };
        assert_eq!(f.niri_bind(&["stop"]), r#"Super+F13 { spawn "riff" "stop"; }"#);
    }
}
