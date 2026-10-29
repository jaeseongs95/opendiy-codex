//! Accept explicit user corrections as pending, redacted consolidation inputs.

use crate::MemoryWorkspaceLock;
use crate::extension_resources::FILENAME_TS_FORMAT;
use codex_secrets::redact_secrets;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use uuid::Uuid;

const MAX_NOTE_BYTES: usize = 16 * 1024;

/// Save a note for later consolidation. Returning a path confirms acceptance only.
pub async fn add_user_memory_note(memory_root: &Path, note: &str) -> io::Result<PathBuf> {
    if note.trim().is_empty() || note.len() > MAX_NOTE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "memory note must be non-empty and at most 16384 UTF-8 bytes",
        ));
    }
    let memory_root = memory_root.to_path_buf();
    let note = redact_secrets(note.to_owned());
    // Keep filesystem operations and the cross-process lock on one blocking worker.
    tokio::task::spawn_blocking(move || save_note(&memory_root, &note))
        .await
        .map_err(io::Error::other)?
}

fn save_note(root: &Path, note: &str) -> io::Result<PathBuf> {
    let mut roots = vec![root.to_path_buf()];
    // A scoped root also owns its namespace ancestors; configured home aliases are outside it.
    if let Some(namespace) = root.ancestors().find(|path| {
        path.file_name()
            .is_some_and(|name| name == "project_memories")
    }) {
        roots = root
            .ancestors()
            .take_while(|path| *path != namespace)
            .map(Path::to_path_buf)
            .collect();
        roots.push(namespace.to_path_buf());
        roots.reverse();
    }
    for directory in &roots {
        ensure_directory(directory)?;
    }
    let lock_path = root.with_extension("lock");
    validate_entry(&lock_path, false, true)?;
    let _lock = MemoryWorkspaceLock::try_acquire(root)?;
    validate_entry(&lock_path, false, false)?;
    let mut directory = root.to_path_buf();
    for component in ["extensions", "ad_hoc", "notes"] {
        directory.push(component);
        ensure_directory(&directory)?;
        roots.push(directory.clone());
    }
    for directory in &roots {
        validate_entry(directory, true, false)?;
    }
    let filename = format!(
        "{}-{}.md",
        chrono::Utc::now().format(FILENAME_TS_FORMAT),
        Uuid::new_v4()
    );
    let path = directory.join(filename);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    if let Err(error) = file.write_all(note.as_bytes()).and_then(|()| file.flush()) {
        drop(file);
        return match fs::remove_file(&path) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(io::Error::other(format!(
                "memory note write failed: {error}; incomplete note cleanup failed: {cleanup}"
            ))),
        };
    }
    Ok(path)
}

fn ensure_directory(path: &Path) -> io::Result<()> {
    validate_entry(path, true, true)?;
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    validate_entry(path, true, false)
}

fn validate_entry(path: &Path, directory: bool, allow_missing: bool) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if allow_missing && error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let redirected = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let redirected = {
        use std::os::windows::fs::MetadataExt;
        redirected || metadata.file_attributes() & 0x400 != 0
    };
    if redirected || (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "memory note path must be a regular {} without symlinks or reparse points: {}",
                if directory { "directory" } else { "file" },
                path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn user_notes_preserve_utf8_use_unique_names_and_redact_before_storage()
    -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp
            .path()
            .join("project_memories")
            .join("a".repeat(64))
            .join("memories_v2");
        let note = "정정: 다음 작업에서는 한국어로 답한다.\n";
        let first = add_user_memory_note(&root, note).await?;
        let second = add_user_memory_note(&root, note).await?;
        assert_ne!(first, second);
        assert_eq!(
            first.parent(),
            Some(root.join("extensions/ad_hoc/notes").as_path())
        );
        assert_eq!(fs::read_to_string(&first)?, note);
        assert_eq!(fs::read_to_string(&second)?, note);
        let filename = first.file_name().unwrap().to_str().unwrap();
        assert!(chrono::NaiveDateTime::parse_from_str(&filename[..19], FILENAME_TS_FORMAT).is_ok());
        assert!(Uuid::parse_str(filename[20..].strip_suffix(".md").unwrap()).is_ok());
        let canary = "sk-abcdefghijklmnopqrstuvwxyz123456";
        let secret = add_user_memory_note(
            &root,
            &format!("정정\napi_key={canary}\nBearer abcdefghijklmnopqrstuv"),
        )
        .await?;
        let stored = fs::read_to_string(secret)?;
        assert!(!stored.contains(canary));
        assert!(!stored.contains("abcdefghijklmnopqrstuv"));
        assert!(stored.contains("[REDACTED_SECRET]"));
        assert!(!root.join("memory_summary.md").exists());
        Ok(())
    }

    #[tokio::test]
    async fn user_notes_reject_empty_and_oversized_utf8_before_creating_roots() -> anyhow::Result<()>
    {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("memories");
        for note in [
            " \n\t".to_owned(),
            "한".repeat(MAX_NOTE_BYTES / 3 + 1),
            "a".repeat(MAX_NOTE_BYTES + 1),
        ] {
            assert_eq!(
                add_user_memory_note(&root, &note).await.unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert!(!root.exists());
        let accepted = add_user_memory_note(&root, &"a".repeat(MAX_NOTE_BYTES)).await?;
        assert_eq!(fs::metadata(accepted)?.len(), MAX_NOTE_BYTES as u64);
        Ok(())
    }

    #[tokio::test]
    async fn user_notes_return_would_block_while_consolidation_owns_root() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("memories");
        fs::create_dir(&root)?;
        let lock = MemoryWorkspaceLock::try_acquire(&root)?;
        assert_eq!(
            add_user_memory_note(&root, "correction")
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(!root.join("extensions").exists());
        drop(lock);
        assert!(add_user_memory_note(&root, "correction").await?.is_file());
        Ok(())
    }

    #[cfg(any(unix, windows))]
    fn redirect_directory(target: &Path, alias: &Path) -> anyhow::Result<()> {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, alias)?;
        #[cfg(windows)]
        {
            let output = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(alias)
                .arg(target)
                .output()?;
            anyhow::ensure!(
                output.status.success(),
                "mklink /J failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    #[cfg(any(unix, windows))]
    #[tokio::test]
    async fn user_notes_reject_redirects_at_every_owned_directory() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let outside = temp.path().join("outside");
        fs::create_dir(&outside)?;
        for level in 0..6 {
            let home = temp.path().join(level.to_string());
            fs::create_dir(&home)?;
            let mut directories = vec![home.join("project_memories")];
            for part in [
                &"b".repeat(64),
                "memories_v2",
                "extensions",
                "ad_hoc",
                "notes",
            ] {
                directories.push(directories.last().unwrap().join(part));
            }
            for parent in &directories[..level] {
                fs::create_dir(parent)?;
            }
            let alias = &directories[level];
            redirect_directory(&outside, alias)?;
            let error = add_user_memory_note(&directories[2], "correction")
                .await
                .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert!(fs::read_dir(&outside)?.next().is_none());
            #[cfg(unix)]
            fs::remove_file(alias)?;
            #[cfg(windows)]
            fs::remove_dir(alias)?;
        }
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn user_notes_reject_redirected_lock_file() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("memories");
        let outside = temp.path().join("outside.lock");
        fs::write(&outside, "canary")?;
        std::os::unix::fs::symlink(&outside, root.with_extension("lock"))?;
        assert_eq!(
            add_user_memory_note(&root, "correction")
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(fs::read_to_string(outside)?, "canary");
        Ok(())
    }
}
