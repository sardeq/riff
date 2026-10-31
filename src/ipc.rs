//! Control socket: lets `riff play "Air horn"` (e.g. bound in niri/Hyprland/Sway)
//! drive the running app, and keeps Riff single-instance.

use crate::core::Core;
use crate::paths::socket_path;
use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

#[cfg(unix)]
pub fn send(cmd: &str) -> std::io::Result<String> {
    use std::os::unix::net::UnixStream;
    let mut s = UnixStream::connect(socket_path())?;
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    s.write_all(cmd.replace('\n', " ").as_bytes())?;
    s.write_all(b"\n")?;
    s.shutdown(std::net::Shutdown::Write)?;
    let mut out = String::new();
    std::io::Read::read_to_string(&mut s, &mut out)?;
    Ok(out)
}

#[cfg(unix)]
pub fn serve(core: Core) {
    use std::os::unix::net::UnixListener;
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            log::warn!("control socket unavailable: {e}");
            return;
        }
    };
    std::thread::Builder::new()
        .name("riff-ipc".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let core = core.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(&stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_ok() {
                        let reply = core.command(&line);
                        let mut w = &stream;
                        let _ = w.write_all(reply.as_bytes());
                        let _ = w.write_all(b"\n");
                    }
                });
            }
        })
        .expect("spawn ipc");
}

pub fn cleanup() {
    let _ = std::fs::remove_file(socket_path());
}

#[cfg(not(unix))]
pub fn send(_cmd: &str) -> std::io::Result<String> {
    Err(std::io::Error::other("remote control is not supported on this platform yet"))
}

#[cfg(not(unix))]
pub fn serve(_core: Core) {}
