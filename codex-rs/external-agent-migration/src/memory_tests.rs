// Modified for OpenDIY Codex R29 (2026-10-30); see MODIFICATIONS.md.
use super::*;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[test]
fn discovers_arbitrary_project_markdown() {
    let root = TempDir::new().expect("create tempdir");
    let external_agent_home = root.path().join(".external-agent");
    let project_root = external_agent_home.join("projects/opaque-project-key");
    let project_memory = project_root.join("memory");
    let project_cwd = root.path().join("project");
    fs::create_dir_all(project_memory.join("topics")).expect("create memory directories");
    fs::create_dir_all(&project_cwd).expect("create project cwd");
    fs::write(
        project_root.join("session.jsonl"),
        serde_json::json!({
            "type": "user",
            "cwd": &project_cwd,
            "timestamp": "2026-07-13T00:00:00Z",
            "message": { "content": "remember this" },
        })
        .to_string(),
    )
    .expect("write session");
    fs::write(project_memory.join("MEMORY.md"), "index").expect("write index");
    fs::write(project_memory.join("release-process.md"), "release notes")
        .expect("write arbitrary topic");
    fs::write(project_memory.join("topics/database.md"), "database notes")
        .expect("write nested topic");
    fs::write(project_memory.join("ignored.txt"), "not markdown").expect("write ignored file");
    let discovered =
        discover_external_memory_files(&external_agent_home).expect("discover memories");
    let project_cwd = fs::canonicalize(project_cwd).expect("canonicalize project cwd");

    assert_eq!(
        discovered
            .iter()
            .map(|memory| {
                (
                    memory.project_key.as_str(),
                    memory.project_cwd.as_deref(),
                    memory.relative_path.as_path(),
                )
            })
            .collect::<Vec<_>>(),
        vec![
            (
                "opaque-project-key",
                Some(project_cwd.as_path()),
                Path::new("MEMORY.md")
            ),
            (
                "opaque-project-key",
                Some(project_cwd.as_path()),
                Path::new("release-process.md")
            ),
            (
                "opaque-project-key",
                Some(project_cwd.as_path()),
                Path::new("topics/database.md")
            ),
        ]
    );
}

#[test]
fn leaves_project_unscoped_without_an_existing_absolute_cwd() {
    let root = TempDir::new().expect("create tempdir");
    let external_agent_home = root.path().join(".external-agent");
    let project_root = external_agent_home.join("projects/opaque-project-key");
    let project_memory = project_root.join("memory");
    fs::create_dir_all(&project_memory).expect("create memory directory");
    fs::write(
        project_root.join("session.jsonl"),
        serde_json::json!({
            "type": "user",
            "cwd": root.path().join("missing-project"),
            "timestamp": "2026-07-13T00:00:00Z",
            "message": { "content": "remember this" },
        })
        .to_string(),
    )
    .expect("write session");
    fs::write(project_memory.join("MEMORY.md"), "index").expect("write index");

    let discovered =
        discover_external_memory_files(&external_agent_home).expect("discover memories");

    assert_eq!(
        discovered,
        vec![ExternalMemoryFile {
            project_key: "opaque-project-key".to_string(),
            project_cwd: None,
            source_path: project_memory.join("MEMORY.md"),
            relative_path: PathBuf::from("MEMORY.md"),
        }]
    );
}

#[cfg(unix)]
#[test]
fn skips_symlinked_memory_directory() {
    use std::os::unix::fs::symlink;

    let root = TempDir::new().expect("create tempdir");
    let external_agent_home = root.path().join(".external-agent");
    let project_root = external_agent_home.join("projects/project");
    let outside_memory = root.path().join("outside-memory");
    fs::create_dir_all(&project_root).expect("create project");
    fs::create_dir_all(&outside_memory).expect("create outside memory");
    fs::write(outside_memory.join("secret.md"), "secret").expect("write outside memory");
    symlink(&outside_memory, project_root.join("memory")).expect("symlink memory directory");

    assert_eq!(
        discover_external_memory_files(&external_agent_home).expect("discover memories"),
        Vec::new()
    );
}

#[test]
fn memory_discovery_caps_file_count_before_returning_a_partial_catalog() -> io::Result<()> {
    let temp = TempDir::new()?;
    let memory = temp.path().join("projects/a/memory");
    fs::create_dir_all(&memory)?;
    for index in 0..MAX_MEMORY_FILES {
        fs::write(memory.join(format!("{index}.md")), "")?;
    }
    assert_eq!(
        discover_external_memory_files(temp.path())?.len(),
        MAX_MEMORY_FILES
    );
    fs::write(memory.join("extra.md"), "")?;
    assert_eq!(
        discover_external_memory_files(temp.path())
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    Ok(())
}

#[test]
fn memory_discovery_caps_file_and_total_bytes_from_metadata() -> io::Result<()> {
    let temp = TempDir::new()?;
    let memory = temp.path().join("projects/a/memory");
    fs::create_dir_all(&memory)?;
    let first = memory.join("0.md");
    fs::File::create(&first)?.set_len(MAX_MEMORY_FILE_BYTES + 1)?;
    assert_eq!(
        discover_external_memory_files(temp.path())
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    fs::File::create(&first)?.set_len(MAX_MEMORY_FILE_BYTES)?;
    for index in 1..16 {
        fs::File::create(memory.join(format!("{index}.md")))?.set_len(MAX_MEMORY_FILE_BYTES)?;
    }
    assert_eq!(discover_external_memory_files(temp.path())?.len(), 16);
    fs::write(memory.join("extra.md"), "x")?;
    assert_eq!(
        discover_external_memory_files(temp.path())
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    Ok(())
}

#[test]
fn memory_discovery_caps_depth_and_directory_allocation() -> io::Result<()> {
    let temp = TempDir::new()?;
    let mut nested = temp.path().join("projects/a/memory");
    for _ in 0..MAX_MEMORY_DEPTH {
        nested.push("d");
    }
    fs::create_dir_all(&nested)?;
    fs::write(nested.join("topic.md"), "topic")?;
    assert_eq!(discover_external_memory_files(temp.path())?.len(), 1);
    fs::create_dir(nested.join("too-deep"))?;
    assert_eq!(
        discover_external_memory_files(temp.path())
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    let mut scanned = MAX_MEMORY_DIRECTORY_ENTRIES;
    assert_eq!(
        bounded_memory_entries(&nested, &mut scanned)
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(scanned, MAX_MEMORY_DIRECTORY_ENTRIES);
    Ok(())
}

#[test]
fn memory_discovery_bounds_session_metadata_reads() -> io::Result<()> {
    let temp = TempDir::new()?;
    let project = temp.path().join("projects/a");
    fs::create_dir_all(project.join("memory"))?;
    fs::write(project.join("memory/MEMORY.md"), "memory")?;
    fs::File::create(project.join("large.jsonl"))?.set_len(MAX_MEMORY_FILE_BYTES + 1)?;
    assert_eq!(
        discover_external_memory_files(temp.path())
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    Ok(())
}
