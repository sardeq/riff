//! Filesystem locations used by Riff (XDG on Linux).

use std::path::PathBuf;
use std::sync::OnceLock;

pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
}

impl Paths {
    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }
    pub fn library_file(&self) -> PathBuf {
        self.config.join("library.json")
    }
    pub fn sounds_dir(&self) -> PathBuf {
        self.data.join("sounds")
    }
    pub fn tts_dir(&self) -> PathBuf {
        self.data.join("tts")
    }
    pub fn voices_dir(&self) -> PathBuf {
        self.data.join("voices")
    }
    pub fn previews_dir(&self) -> PathBuf {
        self.cache.join("previews")
    }
}

pub fn paths() -> &'static Paths {
    static P: OnceLock<Paths> = OnceLock::new();
    P.get_or_init(|| {
        let (config, data, cache) = match directories::ProjectDirs::from("dev", "sardeq", "riff") {
            Some(d) => (d.config_dir().to_path_buf(), d.data_dir().to_path_buf(), d.cache_dir().to_path_buf()),
            None => {
                let base = std::env::temp_dir().join("riff");
                (base.join("config"), base.join("data"), base.join("cache"))
            }
        };
        let p = Paths { config, data, cache };
        for dir in [p.config.clone(), p.sounds_dir(), p.tts_dir(), p.voices_dir(), p.previews_dir()] {
            let _ = std::fs::create_dir_all(dir);
        }
        p
    })
}

/// Path of the control socket used by `riff <command>` and compositor keybinds.
pub fn socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("riff.sock")
}
