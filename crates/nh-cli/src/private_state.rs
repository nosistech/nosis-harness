use nh_law::{read_guarded, GuardedRead};
use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

pub(crate) enum PrivateStateRead {
    Absent,
    Text(String),
    Interrupted,
}

pub(crate) struct PrivateStateFile {
    directory: PathBuf,
    path: PathBuf,
    stem: String,
    max_bytes: usize,
}

impl PrivateStateFile {
    pub(crate) fn open(
        home: &Path,
        subdirectory: Option<&str>,
        filename: &str,
        stem: &str,
        max_bytes: usize,
        create: bool,
    ) -> anyhow::Result<Option<Self>> {
        let Some(nosis) = inspect_directory(&home.join(".nosis"), create)? else {
            return Ok(None);
        };
        let directory = match subdirectory {
            Some(name) => match inspect_directory(&nosis.join(name), create)? {
                Some(directory) => directory,
                None => return Ok(None),
            },
            None => nosis,
        };
        Ok(Some(Self {
            path: directory.join(filename),
            directory,
            stem: stem.to_string(),
            max_bytes,
        }))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn read(&self) -> anyhow::Result<PrivateStateRead> {
        match read_guarded(&self.path, Some(&self.directory), self.max_bytes) {
            GuardedRead::Text(text) => Ok(PrivateStateRead::Text(text)),
            GuardedRead::Absent if self.interrupted_update_present()? => {
                Ok(PrivateStateRead::Interrupted)
            }
            GuardedRead::Absent => Ok(PrivateStateRead::Absent),
            GuardedRead::Refused(reason) => anyhow::bail!("refused private state file: {reason}"),
        }
    }

    pub(crate) fn publish(
        &self,
        expected_previous: Option<&str>,
        replacement: &str,
    ) -> anyhow::Result<()> {
        if replacement.len() > self.max_bytes {
            anyhow::bail!(
                "private state exceeds the {} byte limit; existing state was not changed",
                self.max_bytes
            )
        }
        self.publish_with(expected_previous, replacement, || Ok(()))
    }

    fn publish_with(
        &self,
        expected_previous: Option<&str>,
        replacement: &str,
        after_remove: impl FnOnce() -> io::Result<()>,
    ) -> anyhow::Result<()> {
        let replacement_stage = self.stage("new", replacement)?;
        let restore_stage = match expected_previous {
            Some(previous) => match self.stage("restore", previous) {
                Ok(path) => Some(path),
                Err(error) => {
                    let _ = fs::remove_file(&replacement_stage);
                    return Err(error.into());
                }
            },
            None => None,
        };

        if let Err(error) = probe_hard_link(&self.directory, &self.stem, &replacement_stage) {
            cleanup_stages(&replacement_stage, restore_stage.as_deref());
            anyhow::bail!(
                "could not verify no-clobber private-state publication before changing the saved value: {error}; check permissions, free space, and filesystem hard-link support"
            )
        }

        if let Some(previous) = expected_previous {
            let unchanged = matches!(
                read_guarded(&self.path, Some(&self.directory), self.max_bytes),
                GuardedRead::Text(ref current) if current == previous
            );
            if !unchanged {
                cleanup_stages(&replacement_stage, restore_stage.as_deref());
                anyhow::bail!("private state changed concurrently; retry the command")
            }
            if let Err(error) = fs::remove_file(&self.path) {
                cleanup_stages(&replacement_stage, restore_stage.as_deref());
                anyhow::bail!("could not replace private state: {error}")
            }
        }

        if let Err(error) = after_remove() {
            return self.rollback_after_failure(
                &replacement_stage,
                restore_stage.as_deref(),
                format!("private-state update was interrupted: {error}"),
            );
        }

        if let Err(error) = fs::hard_link(&replacement_stage, &self.path) {
            return self.rollback_after_failure(
                &replacement_stage,
                restore_stage.as_deref(),
                format!("could not publish private state without clobbering: {error}"),
            );
        }
        cleanup_stages(&replacement_stage, restore_stage.as_deref());
        Ok(())
    }

    fn rollback_after_failure(
        &self,
        replacement_stage: &Path,
        restore_stage: Option<&Path>,
        primary: String,
    ) -> anyhow::Result<()> {
        let rollback = restore_stage.map(|restore| fs::hard_link(restore, &self.path));
        let preserve_restore = matches!(rollback, Some(Err(_)));
        cleanup_stages(
            replacement_stage,
            if preserve_restore {
                None
            } else {
                restore_stage
            },
        );
        match rollback {
            Some(Ok(())) => anyhow::bail!("{primary}; previous value restored"),
            Some(Err(error)) => anyhow::bail!(
                "{primary}; restore failed: {error}; recovery bytes remain at {}",
                restore_stage
                    .expect("failed rollback has a restore stage")
                    .display()
            ),
            None => anyhow::bail!("{primary}"),
        }
    }

    pub(crate) fn remove(&self, expected: &str) -> anyhow::Result<()> {
        let unchanged = matches!(
            read_guarded(&self.path, Some(&self.directory), self.max_bytes),
            GuardedRead::Text(ref current) if current == expected
        );
        if !unchanged {
            anyhow::bail!("private state changed concurrently; retry the command")
        }
        fs::remove_file(&self.path)?;
        Ok(())
    }

    fn interrupted_update_present(&self) -> anyhow::Result<bool> {
        let new_prefix = format!(".{}.nh-new-", self.stem);
        let restore_prefix = format!(".{}.nh-restore-", self.stem);
        for entry in fs::read_dir(&self.directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.ends_with(".tmp")
                && (name.starts_with(&new_prefix) || name.starts_with(&restore_prefix))
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn stage(&self, purpose: &str, text: &str) -> io::Result<PathBuf> {
        for attempt in 0..64u8 {
            let path = self.directory.join(format!(
                ".{}.nh-{purpose}-{}-{attempt}.tmp",
                self.stem,
                std::process::id()
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(mut file) => {
                    if let Err(error) = file
                        .write_all(text.as_bytes())
                        .and_then(|()| file.sync_all())
                    {
                        let _ = fs::remove_file(&path);
                        return Err(error);
                    }
                    return Ok(path);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique private-state staging file",
        ))
    }

    #[cfg(test)]
    pub(crate) fn publish_with_test_hook(
        &self,
        expected_previous: Option<&str>,
        replacement: &str,
        after_remove: impl FnOnce() -> io::Result<()>,
    ) -> anyhow::Result<()> {
        if replacement.len() > self.max_bytes {
            anyhow::bail!(
                "private state exceeds the {} byte limit; existing state was not changed",
                self.max_bytes
            )
        }
        self.publish_with(expected_previous, replacement, after_remove)
    }
}

fn inspect_directory(path: &Path, create: bool) -> anyhow::Result<Option<PathBuf>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {
            Ok(Some(path.to_path_buf()))
        }
        Ok(_) => anyhow::bail!(
            "refused private state: {} must be a directory; symlinks and special files are not accepted",
            path.display()
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound && create => {
            match create_private_directory(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => anyhow::bail!(
                    "could not create private-state directory {}: {error}",
                    path.display()
                ),
            }
            let metadata = fs::symlink_metadata(path)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                anyhow::bail!(
                    "refused private state: {} must be a directory; symlinks and special files are not accepted",
                    path.display()
                )
            }
            Ok(Some(path.to_path_buf()))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => anyhow::bail!("could not inspect {}: {error}", path.display()),
    }
}

fn create_private_directory(path: &Path) -> io::Result<()> {
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        let mut builder = builder;
        builder.mode(0o700);
        builder.create(path)
    }
    #[cfg(not(unix))]
    {
        builder.create(path)
    }
}

fn probe_hard_link(directory: &Path, stem: &str, source: &Path) -> io::Result<()> {
    for attempt in 0..64u8 {
        let probe = directory.join(format!(
            ".{stem}.nh-link-probe-{}-{attempt}.tmp",
            std::process::id()
        ));
        match fs::hard_link(source, &probe) {
            Ok(()) => {
                fs::remove_file(probe)?;
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique hard-link probe",
    ))
}

fn cleanup_stages(replacement: &Path, restore: Option<&Path>) {
    let _ = fs::remove_file(replacement);
    if let Some(restore) = restore {
        let _ = fs::remove_file(restore);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_replacement_preserves_existing_bytes_without_staging() {
        let home = tempfile::tempdir().unwrap();
        let state = PrivateStateFile::open(home.path(), None, "sample", "sample", 8, true)
            .unwrap()
            .unwrap();
        fs::write(state.path(), "original").unwrap();

        let error = state
            .publish(Some("original"), "replacement")
            .unwrap_err()
            .to_string();

        assert!(error.contains("8 byte limit"));
        assert_eq!(fs::read_to_string(state.path()).unwrap(), "original");
        assert_eq!(
            fs::read_dir(state.path().parent().unwrap())
                .unwrap()
                .count(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn newly_created_directories_and_files_are_private() {
        use std::os::unix::fs::PermissionsExt as _;

        let home = tempfile::tempdir().unwrap();
        let state = PrivateStateFile::open(
            home.path(),
            Some("mcp-reviews"),
            "sample.json",
            "sample",
            64,
            true,
        )
        .unwrap()
        .unwrap();
        state.publish(None, "{}\n").unwrap();

        let nosis_mode = fs::metadata(home.path().join(".nosis"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let review_mode = fs::metadata(state.path().parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(state.path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(nosis_mode, 0o700);
        assert_eq!(review_mode, 0o700);
        assert_eq!(file_mode, 0o600);
    }
}
