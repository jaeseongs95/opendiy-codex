//! Project identity and artifact paths shared by memory readers and writers.
use codex_git_utils::repository_identity;
use codex_utils_absolute_path::AbsolutePathBuf;
use sha2::Digest;
use sha2::Sha256;
use std::io;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryProject {
    pub key: String,
    identity: AbsolutePathBuf,
    git: bool,
    markers: Vec<String>,
}

impl MemoryProject {
    pub fn resolve(cwd: &Path, markers: &[String]) -> io::Result<Self> {
        let cwd = AbsolutePathBuf::from_absolute_path_checked(cwd)?.canonicalize()?;
        let (identity, git) = if let Some(repo) = repository_identity(cwd.as_path()) {
            (repo.common_dir, true)
        } else {
            let mut project = cwd.clone();
            for ancestor in cwd.ancestors() {
                if ancestor.join(".git").as_path().exists() {
                    return Err(io::Error::other(
                        "project memory requires validated Git metadata",
                    ));
                }
                if markers
                    .iter()
                    .any(|marker| ancestor.join(marker).as_path().exists())
                {
                    project = ancestor;
                    break;
                }
            }
            (project, false)
        };
        let mut hash = Sha256::new();
        hash.update(if git {
            b"git\0".as_slice()
        } else {
            b"dir\0".as_slice()
        });
        hash.update(identity.as_path().as_os_str().as_encoded_bytes());
        let key = format!("{:x}", hash.finalize());
        Ok(Self {
            key,
            identity,
            git,
            markers: markers.to_vec(),
        })
    }

    pub fn markers(&self) -> &[String] {
        &self.markers
    }

    pub fn contains_source(&self, cwd: &Path) -> bool {
        Self::resolve(cwd, &self.markers)
            .is_ok_and(|project| project.git == self.git && project.identity == self.identity)
    }
}

impl crate::types::MemoriesConfig {
    /// None means project identity is unavailable; never fall back to global memory.
    pub fn memory_root(&self, codex_home: &AbsolutePathBuf) -> Option<AbsolutePathBuf> {
        use crate::types::MemoryScope;
        match self.scope {
            MemoryScope::Global => Some(codex_home.join(self.version.directory_name())),
            MemoryScope::Project => self.project.as_ref().map(|project| {
                codex_home
                    .join("project_memories")
                    .join(&project.key)
                    .join(self.version.directory_name())
            }),
        }
    }
}

/// Bound disk input independently of the smaller model-visible token budget.
pub const MEMORY_SUMMARY_MAX_BYTES: u64 = 1024 * 1024;

/// Validate the namespace we own while allowing an explicitly configured home alias.
/// This rejects existing redirects; it does not promise protection from concurrent hostile renames.
pub fn validate_scoped_memory_root(root: &Path) -> io::Result<()> {
    let Some(key) = root.parent() else {
        return Ok(());
    };
    let Some(namespace) = key.parent().filter(|path| {
        path.file_name()
            .is_some_and(|name| name == "project_memories")
    }) else {
        return Ok(());
    };
    for path in [namespace, key, root, &root.join("memory_summary.md")] {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let redirected = metadata.file_type().is_symlink();
        #[cfg(windows)]
        let redirected = {
            use std::os::windows::fs::MetadataExt;
            redirected || metadata.file_attributes() & 0x400 != 0
        };
        if redirected
            || if path == root.join("memory_summary.md") {
                !metadata.is_file()
            } else {
                !metadata.is_dir()
            }
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "project memory path is redirected or has the wrong type: {}",
                    path.display()
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MemoriesConfig, MemoryScope};

    fn git(cwd: &Path, args: &[&str]) {
        let result = std::process::Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[test]
    fn project_memory_worktree_shares_identity_and_clone_is_separate() {
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "-q", "primary"]);
        let primary = temp.path().join("primary");
        git(
            &primary,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "fixture",
            ],
        );
        let linked = temp.path().join("linked");
        git(
            &primary,
            &["worktree", "add", "-qb", "linked", linked.to_str().unwrap()],
        );
        git(
            temp.path(),
            &["clone", "-q", primary.to_str().unwrap(), "clone"],
        );
        let clone = temp.path().join("clone");
        std::fs::create_dir(primary.join("nested")).unwrap();
        let markers = vec![".git".to_string()];
        let project = MemoryProject::resolve(&primary, &markers).unwrap();
        assert_eq!(project, MemoryProject::resolve(&linked, &markers).unwrap());
        assert!(project.contains_source(&primary.join("nested")));
        assert!(!project.contains_source(&clone));
        assert!(!project.contains_source(&temp.path().join("missing")));
        assert_eq!(project.key.len(), 64);
        assert!(project.key.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let mut config = MemoriesConfig {
            scope: MemoryScope::Project,
            project: Some(project),
            ..Default::default()
        };
        let home = AbsolutePathBuf::from_absolute_path_checked(temp.path()).unwrap();
        let root = config.memory_root(&home).unwrap();
        assert!(
            root.as_path()
                .starts_with(home.join("project_memories").as_path())
        );
        config.project = None;
        assert_eq!(
            config.memory_root(&home),
            None,
            "missing identity must not select global memory"
        );
    }

    #[test]
    fn project_memory_marker_and_nested_repository_boundaries() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join(".project"), "").unwrap();
        std::fs::create_dir_all(temp.path().join("nested/deeper")).unwrap();
        let markers = vec![".project".to_string()];
        let project = MemoryProject::resolve(temp.path(), &markers).unwrap();
        assert!(project.contains_source(&temp.path().join("nested/deeper")));
        git(&temp.path().join("nested"), &["init", "-q", "independent"]);
        assert!(!project.contains_source(&temp.path().join("nested/independent")));
        std::fs::write(temp.path().join("nested/deeper/.git"), "gitdir: missing\n").unwrap();
        assert!(MemoryProject::resolve(&temp.path().join("nested/deeper"), &markers).is_err());
    }
}
