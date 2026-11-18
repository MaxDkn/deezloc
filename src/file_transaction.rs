// Journal de restauration pour publier ensemble les fichiers audio et les CSV.
use crate::{config::MUSIC_DIR, Result};
use fs2::FileExt;
use serde_json::{json, Value};
use std::{fs, path::{Path, PathBuf}};

fn journal_path() -> PathBuf { Path::new(MUSIC_DIR).join(".rename-journal.json") }

fn replace(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination.parent().ok_or("Destination sans dossier.")?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    fs::copy(source, temporary.path())?;
    temporary.as_file().sync_all()?;
    temporary.persist(destination)?;
    Ok(())
}

fn remove(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn restore(plan: &Value) -> Result<()> {
    let directory = Path::new(plan["directory"].as_str().ok_or("Journal de renommage invalide.")?);
    for (index, entry) in plan["entries"].as_array().ok_or("Journal de renommage invalide.")?.iter().enumerate() {
        let path = Path::new(entry["path"].as_str().ok_or("Chemin de restauration invalide.")?);
        if entry["existed"].as_bool() == Some(true) {
            replace(&directory.join(format!("{index}.old")), path)?;
        } else { remove(path)?; }
    }
    remove(&journal_path())?;
    let _ = fs::remove_dir_all(directory);
    Ok(())
}

pub(crate) fn recover() -> Result<()> {
    if !journal_path().try_exists()? { return Ok(()); }
    let lock = fs::OpenOptions::new().create(true).truncate(false).write(true)
        .open(Path::new(MUSIC_DIR).join(".lock"))?;
    FileExt::try_lock_exclusive(&lock)?;
    let plan = serde_json::from_slice(&fs::read(journal_path())?)?;
    restore(&plan)
}

// L'appelant conserve le verrou de bibliothèque durant toute l'opération.
pub(crate) fn apply(changes: &[(PathBuf, Option<PathBuf>)]) -> Result<()> {
    if journal_path().try_exists()? {
        return Err("Un renommage interrompu attend sa restauration. Relance DeezLoc.".into());
    }
    let temporary = tempfile::tempdir_in(MUSIC_DIR)?;
    let directory = fs::canonicalize(temporary.path())?;
    let mut entries = Vec::new();
    for (index, (target, replacement)) in changes.iter().enumerate() {
        let path = std::env::current_dir()?.join(target);
        let existed = path.try_exists()?;
        if existed {
            fs::copy(&path, directory.join(format!("{index}.old")))?;
        }
        if let Some(source) = replacement {
            fs::copy(source, directory.join(format!("{index}.new")))?;
        }
        entries.push(json!({"path": path, "existed": existed, "replacement": replacement.is_some()}));
    }
    let plan = json!({"directory": directory, "entries": entries});
    let journal = tempfile::NamedTempFile::new_in(MUSIC_DIR)?;
    fs::write(journal.path(), serde_json::to_vec_pretty(&plan)?)?;
    journal.as_file().sync_all()?;
    journal.persist(journal_path())?;
    // Conserver les sauvegardes, y compris si la restauration échoue.
    let _retained = temporary.keep();
    let publish = (|| -> Result<()> {
        for (index, (path, replacement)) in changes.iter().enumerate() {
            if replacement.is_some() {
                replace(&directory.join(format!("{index}.new")), path)?;
            } else { remove(path)?; }
        }
        remove(&journal_path())?;
        Ok(())
    })();
    if let Err(error) = publish {
        if let Err(rollback) = restore(&plan) {
            return Err(format!("Renommage : {error}. Restauration à terminer au prochain démarrage : {rollback}").into());
        }
        return Err(format!("Renommage annulé : {error}").into());
    }
    let _ = fs::remove_dir_all(directory);
    Ok(())
}
