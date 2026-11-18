use crate::Result;
use csv::{ReaderBuilder, Trim};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs, path::{Path, PathBuf}};

#[derive(Clone)]
pub(crate) struct Track {
    pub(crate) artist: String,
    pub(crate) title: String,
}

impl Track {
    pub(crate) fn key(&self) -> String {
        let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        let key = format!("{}\0{}", normalize(&self.artist), normalize(&self.title));
        format!("{:x}", Sha256::digest(key.as_bytes()))
    }

    pub(crate) fn filename(&self) -> String {
        let raw = format!("{} - {}", self.artist, self.title);
        let mut label = String::new();
        for c in raw.chars() {
            let c = if c.is_control() || r#"<>:"/\|?*%"#.contains(c) { '_' } else { c };
            if label.len() + c.len_utf8() > 160 { break; }
            label.push(c);
        }
        format!("{} [{}].opus", label.trim_end(), &self.key()[..16])
    }
}

pub(crate) fn read_tracks(path: &Path, limit: Option<usize>) -> Result<Vec<Track>> {
    let edits = crate::track_edits::load()?;
    let bytes = fs::read(path)?;
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    for delimiter in [b',', b';', b'\t'] {
        let mut reader = ReaderBuilder::new().delimiter(delimiter).trim(Trim::All).from_reader(bytes);
        let Ok(headers) = reader.headers() else { continue };
        let index = |name: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(name));
        let (Some(t), Some(a)) = (index("Track name"), index("Artist name")) else { continue };
        let mut seen = HashSet::new();
        let mut tracks = Vec::new();
        for (i, row) in reader.records().take(limit.unwrap_or(usize::MAX)).enumerate() {
            let row = row?;
            let track = Track { title: row[t].to_owned(), artist: row[a].to_owned() };
            if track.title.is_empty() || track.artist.is_empty() {
                return Err(format!("Entrée {} : titre ou artiste vide.", i + 1).into());
            }
            let track = crate::track_edits::resolve(track, &edits)?;
            if seen.insert(track.key()) { tracks.push(track); }
        }
        return Ok(tracks);
    }
    Err("En-têtes attendus : Track name et Artist name (CSV UTF-8).".into())
}

#[derive(Clone)]
pub(crate) struct Song {
    pub(crate) track: Track,
    pub(crate) label: String,
    pub(crate) path: PathBuf,
}
