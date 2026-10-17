use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub(crate) struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn new(root: &Path) -> Result<Self> {
        let root = root
            .canonicalize()
            .with_context(|| format!("failed to resolve workspace: {}", root.display()))?;

        if !root.is_dir() {
            bail!("workspace must be a directory: {}", root.display());
        }

        Ok(Self { root })
    }

    /// Resolve an existing path and reject targets outside the workspace.
    /// This is an application boundary check, not protection against concurrent
    /// filesystem changes between validation and use.
    pub(crate) fn resolve_existing(&self, path: &Path) -> Result<PathBuf> {
        let candidate = self.root.join(path);
        let resolved = candidate
            .canonicalize()
            .with_context(|| format!("failed to resolve workspace path: {}", path.display()))?;

        if !resolved.starts_with(&self.root) {
            bail!("path is outside workspace: {}", path.display());
        }

        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_existing_directory() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();

        assert_eq!(workspace.root, directory.path().canonicalize().unwrap());
    }

    #[test]
    fn rejects_missing_path() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("missing");
        let error = Workspace::new(&missing).err().unwrap();

        assert!(error.to_string().contains("failed to resolve workspace"));
        assert!(error.to_string().contains(&missing.display().to_string()));
        assert!(error.chain().nth(1).is_some());
    }

    #[test]
    fn rejects_regular_file() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let error = Workspace::new(file.path()).err().unwrap();

        assert!(error.to_string().contains("workspace must be a directory"));
    }

    #[test]
    fn resolves_root_relative_and_absolute_paths() {
        let directory = tempfile::tempdir().unwrap();
        let child = directory.path().join("src");
        std::fs::create_dir(&child).unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();

        assert_eq!(
            workspace.resolve_existing(Path::new(".")).unwrap(),
            workspace.root
        );
        for path in [Path::new("src"), child.as_path()] {
            assert_eq!(
                workspace.resolve_existing(path).unwrap(),
                child.canonicalize().unwrap()
            );
        }
    }

    #[test]
    fn rejects_missing_target_with_context() {
        let directory = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(directory.path()).unwrap();
        let error = workspace
            .resolve_existing(Path::new("missing"))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("failed to resolve workspace path")
        );
        assert!(error.chain().nth(1).is_some());
    }

    #[test]
    fn rejects_parent_absolute_and_similar_prefix_escape() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        let sibling = directory.path().join("project-backup");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&sibling).unwrap();
        let workspace = Workspace::new(&root).unwrap();

        for path in [
            Path::new(".."),
            Path::new("../project-backup"),
            sibling.as_path(),
        ] {
            let error = workspace.resolve_existing(path).unwrap_err();
            assert!(error.to_string().contains("path is outside workspace"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn checks_symlink_targets_against_workspace_boundary() {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        let inside = root.join("src");
        let outside = directory.path().join("outside");
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::create_dir(&outside).unwrap();
        symlink(&inside, root.join("internal-link")).unwrap();
        symlink(&outside, root.join("external-link")).unwrap();
        let workspace = Workspace::new(&root).unwrap();

        assert_eq!(
            workspace
                .resolve_existing(Path::new("internal-link"))
                .unwrap(),
            inside.canonicalize().unwrap()
        );
        let error = workspace
            .resolve_existing(Path::new("external-link"))
            .unwrap_err();
        assert!(error.to_string().contains("path is outside workspace"));
    }
}
