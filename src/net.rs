//! Tiny blocking HTTP helpers (run them off the UI thread).

use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

pub fn agent() -> &'static ureq::Agent {
    static A: OnceLock<ureq::Agent> = OnceLock::new();
    A.get_or_init(|| {
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_body(Some(Duration::from_secs(120)))
            .user_agent(concat!("Riff/", env!("CARGO_PKG_VERSION"), " (+https://github.com/sardeq/riff)"))
            .build()
            .new_agent()
    })
}

pub fn get_bytes(url: &str) -> Result<Vec<u8>> {
    let mut res = agent().get(url).call().with_context(|| format!("GET {url}"))?;
    let mut out = Vec::new();
    res.body_mut().as_reader().read_to_end(&mut out)?;
    Ok(out)
}

/// Download to `dest` (via a temp file), reporting progress as 0..=1000 permille.
pub fn download(url: &str, dest: &Path, progress: Option<&AtomicU32>) -> Result<()> {
    let mut res = agent().get(url).call().with_context(|| format!("GET {url}"))?;
    let total = res.body().content_length().unwrap_or(0);
    let tmp = dest.with_extension("part");
    let mut file = std::fs::File::create(&tmp)?;
    let mut reader = res.body_mut().as_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        if let (Some(p), true) = (progress, total > 0) {
            p.store((done * 1000 / total) as u32, Ordering::Relaxed);
        }
    }
    file.flush()?;
    drop(file);
    std::fs::rename(&tmp, dest)?;
    Ok(())
}
