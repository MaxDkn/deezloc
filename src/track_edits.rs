use crate::{config::MUSIC_DIR, track::Track, Result};
use serde_json::{json, Map, Value};
use std::{collections::HashSet, fs, path::{Path, PathBuf}};

pub(crate) fn path() -> PathBuf { Path::new(MUSIC_DIR).join(".track-edits.json") }

pub(crate) fn load() -> Result<Map<String, Value>> {
    match fs::read(path()) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn resolve(mut track: Track, edits: &Map<String, Value>) -> Result<Track> {
    let mut seen = HashSet::new();
    loop {
        let key = track.key();
        let Some(value) = edits.get(&key) else { return Ok(track); };
        if !seen.insert(key.clone()) { return Err("Cycle dans les corrections de titres.".into()); }
        let updated = Track {
            title: value["title"].as_str().ok_or("Correction de titre invalide.")?.to_owned(),
            artist: value["artist"].as_str().ok_or("Correction d’artiste invalide.")?.to_owned(),
        };
        if updated.key() == key { return Ok(updated); }
        track = updated;
    }
}

pub(crate) fn record(edits: &mut Map<String, Value>, old: &Track, new: &Track) -> Result<()> {
    let mut aliases = Vec::new();
    for (key, value) in edits.iter() {
        let item = Track {
            title: value["title"].as_str().ok_or("Correction invalide.")?.to_owned(),
            artist: value["artist"].as_str().ok_or("Correction invalide.")?.to_owned(),
        };
        if resolve(item, edits)?.key() == old.key() { aliases.push(key.clone()); }
    }
    let value = json!({"title": new.title, "artist": new.artist});
    edits.remove(&new.key());
    for key in aliases { edits.insert(key, value.clone()); }
    edits.insert(old.key(), value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track(title: &str) -> Track { Track { title: title.into(), artist: "Artiste".into() } }

    #[test]
    fn successive_edits_and_return_to_original_name() {
        let (a, b, c) = (track("Original"), track("Correction"), track("Final"));
        let mut edits = Map::new();
        record(&mut edits, &a, &b).unwrap();
        record(&mut edits, &b, &c).unwrap();
        assert_eq!(resolve(a.clone(), &edits).unwrap().title, c.title);
        assert_eq!(resolve(b.clone(), &edits).unwrap().title, c.title);
        record(&mut edits, &c, &a).unwrap();
        assert_eq!(resolve(b, &edits).unwrap().title, a.title);
        assert_eq!(resolve(c, &edits).unwrap().title, a.title);
    }

    #[test]
    fn case_only_edit_is_preserved() {
        let mut edits = Map::new();
        record(&mut edits, &track("TITLE"), &track("Title")).unwrap();
        assert_eq!(resolve(track("TITLE"), &edits).unwrap().title, "Title");
    }
}
