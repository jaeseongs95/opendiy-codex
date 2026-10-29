use crate::sessions::records_cla::summarize_session_from_reader;
use std::fs;
use std::io;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

pub(crate) const MAX_MEMORY_FILES: usize = 1_000;
pub(crate) const MAX_MEMORY_FILE_BYTES: u64 = 1024 * 1024;
pub(crate) const MAX_MEMORY_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_MEMORY_DEPTH: usize = 16;
const MAX_MEMORY_DIRECTORY_ENTRIES: usize = 10_000;

#[derive(Default)]
pub(crate) struct MemoryReadBudget {
    files: usize,
    bytes: u64,
}

impl MemoryReadBudget {
    pub(crate) fn include(&mut self, bytes: u64) -> io::Result<()> {
        if self.files >= MAX_MEMORY_FILES
            || bytes > MAX_MEMORY_FILE_BYTES
            || bytes > MAX_MEMORY_TOTAL_BYTES.saturating_sub(self.bytes)
        {
            return Err(memory_limit_error(
                "file count or byte budget exceeded (1000 files, 1 MiB/file, 16 MiB total)",
            ));
        }
        self.files += 1;
        self.bytes += bytes;
        Ok(())
    }
}

pub(crate) fn memory_limit_error(message: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("external memory import limit: {message}"),
    )
}

pub(crate) fn read_memory_file(path: &Path, budget: &mut MemoryReadBudget) -> io::Result<Vec<u8>> {
    if budget.files >= MAX_MEMORY_FILES {
        return Err(memory_limit_error("file count exceeds 1000"));
    }
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(memory_limit_error("memory input must be a regular file"));
    }
    let file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(memory_limit_error("memory input must be a regular file"));
    }
    if metadata.len() > MAX_MEMORY_FILE_BYTES {
        return Err(memory_limit_error("file exceeds 1 MiB"));
    }
    let remaining = MAX_MEMORY_TOTAL_BYTES.saturating_sub(budget.bytes);
    if metadata.len() > remaining {
        return Err(memory_limit_error("total bytes exceed 16 MiB"));
    }
    let limit = MAX_MEMORY_FILE_BYTES.min(remaining);
    // Never trust metadata for allocation or growth while reading the file.
    let mut content = Vec::new();
    file.take(limit + 1).read_to_end(&mut content)?;
    budget.include(content.len() as u64)?;
    Ok(content)
}

pub(crate) fn bounded_memory_entries(
    path: &Path,
    scanned: &mut usize,
) -> io::Result<Vec<fs::DirEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(path)? {
        if *scanned >= MAX_MEMORY_DIRECTORY_ENTRIES {
            return Err(memory_limit_error("directory scan exceeds 10000 entries"));
        }
        *scanned += 1;
        entries.push(entry?);
    }
    entries.sort_by_key(fs::DirEntry::file_name);
    Ok(entries)
}

const EXTERNAL_PROJECTS_SUBDIR: &str = "projects";
const EXTERNAL_MEMORY_SUBDIR: &str = "memory";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalMemoryFile {
    pub project_key: String,
    pub project_cwd: Option<PathBuf>,
    pub source_path: PathBuf,
    pub relative_path: PathBuf,
}

/// Discovers every Markdown file in each external-agent project's memory directory.
pub fn discover_external_memory_files(
    external_agent_home: &Path,
) -> io::Result<Vec<ExternalMemoryFile>> {
    let mut files = Vec::new();
    discover_project_memory(external_agent_home, &mut files)?;

    files.sort_by(|left, right| {
        left.project_key
            .cmp(&right.project_key)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
            .then_with(|| left.source_path.cmp(&right.source_path))
    });
    Ok(files)
}

fn discover_project_memory(
    external_agent_home: &Path,
    files: &mut Vec<ExternalMemoryFile>,
) -> io::Result<()> {
    let projects_root = external_agent_home.join(EXTERNAL_PROJECTS_SUBDIR);
    let projects_metadata = match fs::symlink_metadata(&projects_root) {
        Ok(projects_metadata) => projects_metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    if !projects_metadata.file_type().is_dir() {
        return Ok(());
    }

    let mut scanned = 0;
    let mut budget = MemoryReadBudget::default();
    let project_entries = bounded_memory_entries(&projects_root, &mut scanned)?;
    for project_entry in project_entries {
        if !project_entry.file_type()?.is_dir() {
            continue;
        }
        let memory_root = project_entry.path().join(EXTERNAL_MEMORY_SUBDIR);
        let memory_metadata = match fs::symlink_metadata(&memory_root) {
            Ok(memory_metadata) => memory_metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        if !memory_metadata.file_type().is_dir() {
            continue;
        }
        let project_key = project_entry.file_name().to_string_lossy().into_owned();
        let project_cwd =
            project_cwd_from_sessions(&project_entry.path(), &mut budget, &mut scanned)?;
        collect_markdown_files(
            &memory_root,
            &memory_root,
            &project_key,
            project_cwd.as_deref(),
            files,
            &mut budget,
            &mut scanned,
            0,
        )?;
    }
    Ok(())
}

fn project_cwd_from_sessions(
    project_root: &Path,
    budget: &mut MemoryReadBudget,
    scanned: &mut usize,
) -> io::Result<Option<PathBuf>> {
    let mut sessions = bounded_memory_entries(project_root, scanned)?
        .into_iter()
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            let path = entry.path();
            if !file_type.is_file()
                || path.extension().and_then(|extension| extension.to_str()) != Some("jsonl")
            {
                return None;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            Some((modified, path))
        })
        .collect::<Vec<_>>();
    sessions.sort_by(|left, right| right.cmp(left));

    for (_, session_path) in sessions {
        let content = match read_memory_file(&session_path, budget) {
            Ok(content) => content,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => return Err(error),
            // Preserve the existing policy of ignoring unavailable session metadata.
            Err(_) => continue,
        };
        if let Ok(Some(summary)) =
            summarize_session_from_reader(&session_path, io::Cursor::new(content))
        {
            let cwd = summary.migration.cwd;
            if !cwd.is_absolute() {
                continue;
            }
            let Ok(cwd) = fs::canonicalize(cwd) else {
                continue;
            };
            if cwd.is_dir() {
                return Ok(Some(cwd));
            }
        }
    }
    Ok(None)
}

fn collect_markdown_files(
    source_root: &Path,
    current_dir: &Path,
    project_key: &str,
    project_cwd: Option<&Path>,
    files: &mut Vec<ExternalMemoryFile>,
    budget: &mut MemoryReadBudget,
    scanned: &mut usize,
    depth: usize,
) -> io::Result<()> {
    if depth > MAX_MEMORY_DEPTH {
        return Err(memory_limit_error("directory depth exceeds 16"));
    }
    let entries = bounded_memory_entries(current_dir, scanned)?;
    for entry in entries {
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            collect_markdown_files(
                source_root,
                &entry.path(),
                project_key,
                project_cwd,
                files,
                budget,
                scanned,
                depth + 1,
            )?;
            continue;
        }
        if !file_type.is_file() || !is_markdown_file(&entry.path()) {
            continue;
        }
        budget.include(entry.metadata()?.len())?;
        let relative_path = entry
            .path()
            .strip_prefix(source_root)
            .map(Path::to_path_buf)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        files.push(ExternalMemoryFile {
            project_key: project_key.to_string(),
            project_cwd: project_cwd.map(Path::to_path_buf),
            source_path: entry.path(),
            relative_path,
        });
    }
    Ok(())
}

fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
