//! Runtime artifact paths that stay inside an explicitly selected root.

use std::path::{Component, Path, PathBuf};

use anyhow::Context as _;

/// Create a relative directory one component at a time and return its canonical path.
/// Existing symlinks/junctions are allowed only when they still resolve inside `root`.
pub fn ensure_contained_dir(root: &Path, relative: &Path) -> anyhow::Result<PathBuf> {
    walk_contained_dir(root, relative, true)?
        .ok_or_else(|| anyhow::anyhow!("runtime directory disappeared while it was created"))
}

/// Resolve an existing relative directory without creating anything.
pub fn resolve_contained_dir(root: &Path, relative: &Path) -> anyhow::Result<Option<PathBuf>> {
    walk_contained_dir(root, relative, false)
}

/// Resolve a file's parent under `root`, creating missing directories, and reject an existing
/// symlink or special file at the final path.
pub fn ensure_contained_file(root: &Path, target: &Path, label: &str) -> anyhow::Result<PathBuf> {
    let relative = target.strip_prefix(root).map_err(|_| {
        anyhow::anyhow!(
            "refused: runtime path {} escapes root {}",
            target.display(),
            root.display()
        )
    })?;
    validate_relative(relative)?;
    let file_name = relative
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("refused: runtime file path has no file name"))?;
    let parent = ensure_contained_dir(root, relative.parent().unwrap_or_else(|| Path::new("")))?;
    let path = parent.join(file_name);
    reject_symlink_or_special_file(&path, label)?;
    Ok(path)
}

/// Refuse link-like and non-regular final paths before an append/open operation.
pub fn reject_symlink_or_special_file(path: &Path, label: &str) -> anyhow::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            anyhow::bail!("refused: {label} path is not a regular file")
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("could not stat {}", path.display())),
    }
}

fn walk_contained_dir(
    root: &Path,
    relative: &Path,
    create: bool,
) -> anyhow::Result<Option<PathBuf>> {
    validate_relative(relative)?;
    let canonical_root = std::fs::canonicalize(root)
        .with_context(|| format!("could not resolve runtime root {}", root.display()))?;
    let mut current = canonical_root.clone();

    for component in relative.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        current.push(name);
        match std::fs::symlink_metadata(&current) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => {
                return Ok(None)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match std::fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!("could not create runtime directory {}", current.display())
                        })
                    }
                }
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("could not inspect runtime directory {}", current.display())
                })
            }
        }

        let resolved = std::fs::canonicalize(&current).with_context(|| {
            format!("could not resolve runtime directory {}", current.display())
        })?;
        if !resolved.starts_with(&canonical_root) {
            anyhow::bail!(
                "refused: runtime directory {} resolves outside root {}",
                current.display(),
                root.display()
            );
        }
        if !resolved.is_dir() {
            anyhow::bail!(
                "refused: runtime directory path is not a directory: {}",
                current.display()
            );
        }
        current = resolved;
    }
    Ok(Some(current))
}

fn validate_relative(relative: &Path) -> anyhow::Result<()> {
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        anyhow::bail!("refused: runtime path must be relative and may not traverse parents");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    #[test]
    fn creates_and_resolves_a_contained_directory() {
        let root = tempfile::tempdir().unwrap();
        let path = ensure_contained_dir(root.path(), Path::new(".nosis/fleet")).unwrap();

        assert!(path.is_dir());
        assert!(path.starts_with(std::fs::canonicalize(root.path()).unwrap()));
        assert_eq!(
            resolve_contained_dir(root.path(), Path::new(".nosis/fleet")).unwrap(),
            Some(path)
        );
    }

    #[test]
    fn refuses_a_parent_link_that_resolves_outside_the_root() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        if symlink_dir(outside.path(), &root.path().join(".nosis")).is_err() {
            return;
        }

        let error = ensure_contained_dir(root.path(), Path::new(".nosis/fleet")).unwrap_err();

        assert!(error.to_string().contains("resolves outside root"));
        assert!(!outside.path().join("fleet").exists());
    }

    #[test]
    fn resolving_a_missing_directory_is_read_only() {
        let root = tempfile::tempdir().unwrap();

        assert_eq!(
            resolve_contained_dir(root.path(), Path::new(".nosis/fleet")).unwrap(),
            None
        );
        assert!(!root.path().join(".nosis").exists());
    }
}
