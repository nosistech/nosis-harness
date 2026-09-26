//! Atomic file staging, publication, and bounded edit conflict checks.

use super::{MAX_TOOL_READ_BYTES, TOOL_BUFFER_BYTES};
use anyhow::{bail, Context};
use std::io::Read;
use std::path::{Path, PathBuf};

/// SECURITY INVARIANT: temporary files are created exclusively in the destination directory.
pub(super) fn create_temp_file(
    parent: &Path,
    prefix: &str,
    nonce: u128,
    path_label: &str,
) -> anyhow::Result<(PathBuf, std::fs::File)> {
    create_temp_file_with_privacy(parent, prefix, nonce, path_label, false)
}

pub(super) fn create_edit_temp_file(
    parent: &Path,
    prefix: &str,
    nonce: u128,
    path_label: &str,
) -> anyhow::Result<(PathBuf, std::fs::File)> {
    create_temp_file_with_privacy(parent, prefix, nonce, path_label, true)
}

fn create_temp_file_with_privacy(
    parent: &Path,
    prefix: &str,
    nonce: u128,
    path_label: &str,
    private_on_unix: bool,
) -> anyhow::Result<(PathBuf, std::fs::File)> {
    let mut attempt = 0_u16;
    loop {
        if attempt == 1000 {
            bail!("could not create temporary file for {path_label}");
        }
        let candidate = parent.join(format!(
            "{prefix}{}-{nonce}-{attempt}.tmp",
            std::process::id()
        ));
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        if private_on_unix {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        #[cfg(not(unix))]
        let _ = private_on_unix;
        match options.open(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                attempt += 1;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("could not create temporary file for {path_label}"))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CreatePublication {
    Published { cleanup_error: Option<String> },
    DestinationExists,
}

fn remove_staged_file(temp_path: &Path, path_label: &str) -> anyhow::Result<()> {
    match std::fs::remove_file(temp_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => bail!("could not clean temporary file for {path_label}: {error}"),
    }
}

/// Publish a synced sibling staging file without ever replacing `destination`.
///
/// `before_publish` exists only so tests can place a competing file at the exact
/// publication boundary. Production passes a no-op closure.
pub(super) fn publish_create_only_with(
    temp_path: &Path,
    destination: &Path,
    path_label: &str,
    before_publish: impl FnOnce() -> anyhow::Result<()>,
) -> anyhow::Result<CreatePublication> {
    if let Err(error) = before_publish() {
        return match remove_staged_file(temp_path, path_label) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(anyhow::anyhow!("{error}; additionally, {cleanup}")),
        };
    }

    match std::fs::hard_link(temp_path, destination) {
        Ok(()) => {
            let cleanup_error = remove_staged_file(temp_path, path_label)
                .err()
                .map(|error| error.to_string());
            Ok(CreatePublication::Published { cleanup_error })
        }
        Err(error) => {
            let destination_exists = error.kind() == std::io::ErrorKind::AlreadyExists
                || std::fs::symlink_metadata(destination).is_ok();
            let cleanup = remove_staged_file(temp_path, path_label);
            if destination_exists {
                return match cleanup {
                    Ok(()) => Ok(CreatePublication::DestinationExists),
                    Err(cleanup) => Err(anyhow::anyhow!(
                        "refused: {path_label} already exists; additionally, {cleanup}"
                    )),
                };
            }
            let failure = anyhow::anyhow!(
                "could not atomically create {path_label} without replacing an existing path: {error} - check permissions, free space, and filesystem hard-link support"
            );
            match cleanup {
                Ok(()) => Err(failure),
                Err(cleanup) => Err(anyhow::anyhow!("{failure}; additionally, {cleanup}")),
            }
        }
    }
}

pub(super) fn publish_create_only(
    temp_path: &Path,
    destination: &Path,
    path_label: &str,
) -> anyhow::Result<CreatePublication> {
    publish_create_only_with(temp_path, destination, path_label, || Ok(()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditPublication {
    Published,
    Conflict,
    Cancelled,
}

fn edit_target_still_matches(
    destination: &Path,
    expected: &[u8],
    expected_metadata: &std::fs::Metadata,
    path_label: &str,
) -> anyhow::Result<bool> {
    let current_metadata = match std::fs::symlink_metadata(destination) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("could not recheck {path_label} before editing"))
        }
    };
    if !current_metadata.file_type().is_file()
        || current_metadata.len() != expected_metadata.len()
        || current_metadata.permissions().readonly() != expected_metadata.permissions().readonly()
    {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if current_metadata.permissions().mode() != expected_metadata.permissions().mode() {
            return Ok(false);
        }
    }
    if let (Ok(expected_modified), Ok(current_modified)) =
        (expected_metadata.modified(), current_metadata.modified())
    {
        if current_modified != expected_modified {
            return Ok(false);
        }
    }

    let mut current = Vec::with_capacity(expected.len().min(TOOL_BUFFER_BYTES));
    std::fs::File::open(destination)
        .with_context(|| format!("could not recheck {path_label} before editing"))?
        .take((MAX_TOOL_READ_BYTES + 1) as u64)
        .read_to_end(&mut current)
        .with_context(|| format!("could not recheck {path_label} before editing"))?;
    Ok(current.len() <= MAX_TOOL_READ_BYTES && current == expected)
}

/// Best-effort edit conflict detection. The comparison catches cooperative edits
/// before publication, but an external writer can still race the final rename.
pub(super) fn publish_edit_with(
    temp_path: &Path,
    destination: &Path,
    expected: &[u8],
    expected_metadata: &std::fs::Metadata,
    path_label: &str,
    before_check: impl FnOnce() -> anyhow::Result<()>,
    cancelled_after_check: impl FnOnce() -> bool,
) -> anyhow::Result<EditPublication> {
    if let Err(error) = before_check() {
        return match remove_staged_file(temp_path, path_label) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(anyhow::anyhow!("{error}; additionally, {cleanup}")),
        };
    }
    let unchanged =
        match edit_target_still_matches(destination, expected, expected_metadata, path_label) {
            Ok(unchanged) => unchanged,
            Err(error) => {
                return match remove_staged_file(temp_path, path_label) {
                    Ok(()) => Err(error),
                    Err(cleanup) => Err(anyhow::anyhow!("{error}; additionally, {cleanup}")),
                };
            }
        };
    if !unchanged {
        return match remove_staged_file(temp_path, path_label) {
            Ok(()) => Ok(EditPublication::Conflict),
            Err(cleanup) => Err(anyhow::anyhow!(
                "refused: {path_label} changed since it was read; additionally, {cleanup}"
            )),
        };
    }
    if cancelled_after_check() {
        return match remove_staged_file(temp_path, path_label) {
            Ok(()) => Ok(EditPublication::Cancelled),
            Err(cleanup) => Err(anyhow::anyhow!(
                "turn cancelled before file edit; additionally, {cleanup}"
            )),
        };
    }
    if let Err(error) = std::fs::rename(temp_path, destination) {
        let failure = anyhow::anyhow!("could not replace {path_label}: {error}");
        return match remove_staged_file(temp_path, path_label) {
            Ok(()) => Err(failure),
            Err(cleanup) => Err(anyhow::anyhow!("{failure}; additionally, {cleanup}")),
        };
    }
    Ok(EditPublication::Published)
}

pub(super) fn publish_edit(
    temp_path: &Path,
    destination: &Path,
    expected: &[u8],
    expected_metadata: &std::fs::Metadata,
    path_label: &str,
    cancelled_after_check: impl FnOnce() -> bool,
) -> anyhow::Result<EditPublication> {
    publish_edit_with(
        temp_path,
        destination,
        expected,
        expected_metadata,
        path_label,
        || Ok(()),
        cancelled_after_check,
    )
}
