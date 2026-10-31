//! Freesound.org search (APIv2, token auth) and preview downloads.

use crate::net;
use crate::paths::paths;
use anyhow::{Result, anyhow};
use serde::Deserialize;
use std::io::Read;
use std::path::PathBuf;

pub const API_KEY_URL: &str = "https://freesound.org/apiv2/apply/";

#[derive(Clone, Debug, Deserialize)]
pub struct Previews {
    #[serde(rename = "preview-hq-mp3")]
    pub hq_mp3: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FsSound {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub duration: f32,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub previews: Previews,
    #[serde(default)]
    pub num_downloads: u64,
    #[serde(default)]
    pub url: String,
}

impl FsSound {
    pub fn license_short(&self) -> &'static str {
        let l = self.license.to_lowercase();
        if l.contains("publicdomain/zero") || l.contains("cc0") {
            "CC0"
        } else if l.contains("by-nc") {
            "CC BY-NC"
        } else if l.contains("sampling+") {
            "Sampling+"
        } else if l.contains("/by/") || l.contains("attribution") {
            "CC BY"
        } else {
            "License"
        }
    }

    pub fn clean_name(&self) -> String {
        let n = self.name.trim();
        let n = n
            .rsplit_once('.')
            .filter(|(_, ext)| ext.len() <= 4 && crate::model::AUDIO_EXTS.contains(&ext.to_lowercase().as_str()))
            .map(|(a, _)| a)
            .unwrap_or(n);
        crate::model::pretty_name(std::path::Path::new(&format!("{n}.x")))
    }
}

#[derive(Deserialize)]
struct Page {
    count: u64,
    results: Vec<FsSound>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Length {
    Short,
    Medium,
    Any,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sort {
    Relevance,
    Downloads,
    Rating,
}

pub fn search(key: &str, query: &str, len: Length, sort: Sort, page: u32) -> Result<(Vec<FsSound>, u64)> {
    if key.trim().is_empty() {
        return Err(anyhow!("Add your free Freesound API key in Settings"));
    }
    let filter = match len {
        Length::Short => "duration:[0 TO 5]",
        Length::Medium => "duration:[0 TO 30]",
        Length::Any => "",
    };
    let sort = match sort {
        Sort::Relevance => "score",
        Sort::Downloads => "downloads_desc",
        Sort::Rating => "rating_desc",
    };
    let mut req = net::agent()
        .get("https://freesound.org/apiv2/search/")
        .query("query", query)
        .query("fields", "id,name,username,duration,license,tags,previews,num_downloads,url")
        .query("page_size", "30")
        .query("page", page.to_string())
        .query("sort", sort)
        .query("token", key.trim());
    if !filter.is_empty() {
        req = req.query("filter", filter);
    }
    let mut res = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(401)) | Err(ureq::Error::StatusCode(403)) => {
            return Err(anyhow!("Freesound rejected the API key — check it in Settings"));
        }
        Err(e) => return Err(anyhow!("Freesound: {e}")),
    };
    let mut bytes = Vec::new();
    res.body_mut().as_reader().read_to_end(&mut bytes)?;
    let page: Page = serde_json::from_slice(&bytes).map_err(|e| anyhow!("unexpected Freesound reply: {e}"))?;
    Ok((page.results, page.count))
}

/// Download the HQ preview (cached by id).
pub fn fetch_preview(s: &FsSound) -> Result<PathBuf> {
    let path = paths().previews_dir().join(format!("fs-{}.mp3", s.id));
    if !path.exists() {
        net::download(&s.previews.hq_mp3, &path, None)?;
    }
    Ok(path)
}
