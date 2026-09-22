//! Operator-owned, user-global default route selection.

use std::fs::{self, OpenOptions};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use nh_law::{read_guarded, GuardedRead};
use nh_routes::RouteResolver;
use nh_vault::Scrubber;

use crate::cmd_run;

pub(crate) const FALLBACK_MODEL: &str = "deepseek-v4-flash";
const MAX_MODEL_BYTES: usize = 256;

pub(crate) fn selected_model(
    explicit: Option<&str>,
    resolver: &RouteResolver,
) -> anyhow::Result<String> {
    let home = nh_law::user_home_dir();
    selected_model_with_home(explicit, resolver, home.as_deref())
}

fn selected_model_with_home(
    explicit: Option<&str>,
    resolver: &RouteResolver,
    home: Option<&Path>,
) -> anyhow::Result<String> {
    if let Some(explicit) = explicit {
        return resolver
            .resolve(explicit)
            .map(|route| route.id().to_owned());
    }

    let saved = match home {
        Some(home) => read_preference(home)?,
        None => None,
    };
    let model = saved.as_deref().unwrap_or(FALLBACK_MODEL);
    match resolver.resolve(model) {
        Ok(route) => Ok(route.id().to_owned()),
        Err(error) if saved.is_some() => anyhow::bail!(
            "saved model preference is not available in the current trusted catalog: {error}; run `nh model set <id>` or `nh model clear`"
        ),
        Err(error) => Err(error),
    }
}

pub(crate) fn set(model: &str) -> anyhow::Result<()> {
    let route = save(model)?;
    print_safe(&format!("saved default model: {route}"));
    print_safe("Explicit --model always takes precedence.");
    Ok(())
}

pub(crate) fn save(model: &str) -> anyhow::Result<String> {
    let cwd = std::env::current_dir()?;
    let (_, catalog) = cmd_run::find_catalog(&cwd)?;
    let resolver = RouteResolver::from_toml(&catalog)?;
    let route = resolver.resolve(model)?;
    let home = required_home()?;
    set_with_home(&home, route.id())?;
    Ok(route.id().to_owned())
}

pub(crate) fn show() -> anyhow::Result<()> {
    let home = required_home()?;
    let Some(saved) = read_preference(&home)? else {
        print_safe(&format!(
            "no saved model preference; default is {FALLBACK_MODEL}"
        ));
        return Ok(());
    };
    let cwd = std::env::current_dir()?;
    let (_, catalog) = cmd_run::find_catalog(&cwd)?;
    let resolver = RouteResolver::from_toml(&catalog)?;
    let selected = selected_model_with_home(None, &resolver, Some(&home))?;
    print_safe(&format!("saved default model: {selected}"));
    print_safe("Explicit --model always takes precedence.");
    debug_assert_eq!(saved, selected);
    Ok(())
}

pub(crate) fn clear() -> anyhow::Result<()> {
    let home = required_home()?;
    if clear_with_home(&home)? {
        print_safe("cleared saved model preference");
    } else {
        print_safe("no saved model preference to clear");
    }
    Ok(())
}

fn print_safe(line: &str) {
    println!("{}", cmd_run::safe_line(&Scrubber::new(Vec::new()), line));
}

fn required_home() -> anyhow::Result<PathBuf> {
    nh_law::user_home_dir().ok_or_else(|| {
        anyhow::anyhow!("home directory is unavailable - cannot manage ~/.nosis/model")
    })
}

fn preference_path(home: &Path) -> PathBuf {
    home.join(".nosis").join("model")
}

fn inspect_preference_dir(home: &Path, create: bool) -> anyhow::Result<Option<PathBuf>> {
    let directory = home.join(".nosis");
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {
            Ok(Some(directory))
        }
        Ok(_) => anyhow::bail!(
            "refused ~/.nosis/model: ~/.nosis must be a directory; symlinks and special files are not accepted"
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound && create => {
            match fs::create_dir(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => anyhow::bail!("could not create ~/.nosis: {error}"),
            }
            let metadata = fs::symlink_metadata(&directory)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                anyhow::bail!(
                    "refused ~/.nosis/model: ~/.nosis must be a directory; symlinks and special files are not accepted"
                )
            }
            Ok(Some(directory))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => anyhow::bail!("could not inspect ~/.nosis: {error}"),
    }
}

fn read_preference(home: &Path) -> anyhow::Result<Option<String>> {
    let Some(directory) = inspect_preference_dir(home, false)? else {
        return Ok(None);
    };
    let path = preference_path(home);
    match read_guarded(&path, Some(home), MAX_MODEL_BYTES) {
        GuardedRead::Text(text) => parse_preference(&text).map(Some),
        GuardedRead::Absent if interrupted_update_present(&directory)? => anyhow::bail!(
            "saved model preference is absent while interrupted update files remain in ~/.nosis; run `nh model set <id>` to choose a model again"
        ),
        GuardedRead::Absent => Ok(None),
        GuardedRead::Refused(reason) => {
            anyhow::bail!("refused saved model preference: {reason}; run `nh model clear`");
        }
    }
}

fn interrupted_update_present(directory: &Path) -> anyhow::Result<bool> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.ends_with(".tmp")
            && (name.starts_with(".model.nh-new-") || name.starts_with(".model.nh-restore-"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn parse_preference(text: &str) -> anyhow::Result<String> {
    let value = text.strip_suffix('\n').unwrap_or(text);
    let value = value.strip_suffix('\r').unwrap_or(value);
    if value.is_empty()
        || value.len() > 128
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        anyhow::bail!(
            "saved model preference is malformed; run `nh model set <id>` or `nh model clear`"
        )
    }
    Ok(value.to_owned())
}

fn set_with_home(home: &Path, model: &str) -> anyhow::Result<()> {
    let model = parse_preference(model)?;
    let directory = inspect_preference_dir(home, true)?.expect("created preference directory");
    let path = preference_path(home);
    let previous = match read_guarded(&path, Some(home), MAX_MODEL_BYTES) {
        GuardedRead::Text(text) => Some(text),
        GuardedRead::Absent => None,
        GuardedRead::Refused(reason) => anyhow::bail!("refused saved model preference: {reason}"),
    };
    let replacement = format!("{model}\n");
    publish_preference(&directory, &path, previous.as_deref(), &replacement)
}

fn publish_preference(
    directory: &Path,
    path: &Path,
    previous: Option<&str>,
    replacement: &str,
) -> anyhow::Result<()> {
    publish_preference_with(directory, path, previous, replacement, || Ok(()))
}

fn publish_preference_with(
    directory: &Path,
    path: &Path,
    previous: Option<&str>,
    replacement: &str,
    after_remove: impl FnOnce() -> io::Result<()>,
) -> anyhow::Result<()> {
    let replacement_stage = stage_preference(directory, "new", replacement)?;
    let restore_stage = match previous {
        Some(previous) => match stage_preference(directory, "restore", previous) {
            Ok(path) => Some(path),
            Err(error) => {
                let _ = fs::remove_file(&replacement_stage);
                return Err(error.into());
            }
        },
        None => None,
    };

    if let Err(error) = probe_hard_link(directory, &replacement_stage) {
        cleanup_preference_stages(&replacement_stage, restore_stage.as_deref());
        anyhow::bail!(
            "could not verify no-clobber preference publication before changing the saved value: {error}; check permissions, free space, and filesystem hard-link support"
        )
    }

    if let Some(previous) = previous {
        let unchanged = matches!(
            read_guarded(path, path.parent(), MAX_MODEL_BYTES),
            GuardedRead::Text(ref current) if current == previous
        );
        if !unchanged {
            cleanup_preference_stages(&replacement_stage, restore_stage.as_deref());
            anyhow::bail!("saved model preference changed concurrently; retry the command")
        }
        if let Err(error) = fs::remove_file(path) {
            cleanup_preference_stages(&replacement_stage, restore_stage.as_deref());
            anyhow::bail!("could not replace saved model preference: {error}")
        }
    }

    if let Err(error) = after_remove() {
        let rollback = restore_stage
            .as_deref()
            .map(|restore| fs::hard_link(restore, path));
        let preserve_restore = matches!(rollback, Some(Err(_)));
        let cleanup_restore = if preserve_restore {
            None
        } else {
            restore_stage.as_deref()
        };
        cleanup_preference_stages(&replacement_stage, cleanup_restore);
        match rollback {
            Some(Ok(())) => anyhow::bail!(
                "saved model preference update was interrupted: {error}; previous value restored"
            ),
            Some(Err(rollback)) => anyhow::bail!(
                "saved model preference update was interrupted: {error}; restore failed: {rollback}; recovery bytes remain at {}",
                restore_stage
                    .as_deref()
                    .expect("previous value has restore stage")
                    .display()
            ),
            None => anyhow::bail!("saved model preference update was interrupted: {error}"),
        }
    }

    if let Err(error) = fs::hard_link(&replacement_stage, path) {
        let rollback = restore_stage
            .as_deref()
            .map(|restore| fs::hard_link(restore, path));
        let preserve_restore = matches!(rollback, Some(Err(_)));
        let cleanup_restore = if preserve_restore {
            None
        } else {
            restore_stage.as_deref()
        };
        cleanup_preference_stages(&replacement_stage, cleanup_restore);
        match rollback {
            Some(Ok(())) => anyhow::bail!(
                "could not publish saved model preference without clobbering: {error}; previous value restored"
            ),
            Some(Err(rollback)) => anyhow::bail!(
                "could not publish saved model preference without clobbering: {error}; restore also failed: {rollback}; recovery bytes remain at {}",
                restore_stage
                    .as_deref()
                    .expect("previous value has restore stage")
                    .display()
            ),
            None => anyhow::bail!(
                "could not publish saved model preference without clobbering: {error}"
            ),
        }
    }
    cleanup_preference_stages(&replacement_stage, restore_stage.as_deref());
    Ok(())
}

fn probe_hard_link(directory: &Path, source: &Path) -> io::Result<()> {
    for attempt in 0..64u8 {
        let probe = directory.join(format!(
            ".model.nh-link-probe-{}-{attempt}.tmp",
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

fn stage_preference(directory: &Path, purpose: &str, text: &str) -> io::Result<PathBuf> {
    for attempt in 0..64u8 {
        let path = directory.join(format!(
            ".model.nh-{purpose}-{}-{attempt}.tmp",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
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
        "could not allocate a unique preference staging file",
    ))
}

fn cleanup_preference_stages(replacement: &Path, restore: Option<&Path>) {
    let _ = fs::remove_file(replacement);
    if let Some(restore) = restore {
        let _ = fs::remove_file(restore);
    }
}

fn clear_with_home(home: &Path) -> anyhow::Result<bool> {
    let Some(_) = inspect_preference_dir(home, false)? else {
        return Ok(false);
    };
    let path = preference_path(home);
    let original = match read_guarded(&path, Some(home), MAX_MODEL_BYTES) {
        GuardedRead::Text(text) => text,
        GuardedRead::Absent => return Ok(false),
        GuardedRead::Refused(reason) => anyhow::bail!("refused saved model preference: {reason}"),
    };
    let unchanged = matches!(
        read_guarded(&path, Some(home), MAX_MODEL_BYTES),
        GuardedRead::Text(ref current) if current == &original
    );
    if !unchanged {
        anyhow::bail!("saved model preference changed concurrently; retry the command")
    }
    fs::remove_file(path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CATALOG: &str = r#"
        [routes.default]
        provider = "test"
        model_id = "default"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"

        [routes.saved]
        provider = "test"
        model_id = "saved"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"

        [routes.explicit]
        provider = "test"
        model_id = "explicit"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
    "#;

    fn resolver() -> RouteResolver {
        RouteResolver::from_toml(TEST_CATALOG).unwrap()
    }

    #[test]
    fn explicit_model_wins_without_reading_broken_preference_path() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join(".nosis")).unwrap();
        fs::create_dir(home.path().join(".nosis").join("model")).unwrap();

        let selected =
            selected_model_with_home(Some("explicit"), &resolver(), Some(home.path())).unwrap();

        assert_eq!(selected, "explicit");
    }

    #[test]
    fn saved_model_is_used_only_after_current_resolver_validation() {
        let home = tempfile::tempdir().unwrap();
        set_with_home(home.path(), "saved").unwrap();

        assert_eq!(
            selected_model_with_home(None, &resolver(), Some(home.path())).unwrap(),
            "saved"
        );

        set_with_home(home.path(), "stale").unwrap();
        let error = selected_model_with_home(None, &resolver(), Some(home.path()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("not available in the current trusted catalog"));
        assert!(error.contains("nh model clear"));
    }

    #[test]
    fn preference_round_trip_is_bounded_and_clear_is_explicit() {
        let home = tempfile::tempdir().unwrap();

        assert_eq!(read_preference(home.path()).unwrap(), None);
        set_with_home(home.path(), "saved").unwrap();
        assert_eq!(
            read_preference(home.path()).unwrap().as_deref(),
            Some("saved")
        );
        assert!(clear_with_home(home.path()).unwrap());
        assert!(!clear_with_home(home.path()).unwrap());
    }

    #[test]
    fn interrupted_absent_preference_never_silently_enables_fallback() {
        let home = tempfile::tempdir().unwrap();
        let directory = inspect_preference_dir(home.path(), true).unwrap().unwrap();
        fs::write(directory.join(".model.nh-restore-123-0.tmp"), "saved\n").unwrap();

        let error = selected_model_with_home(None, &resolver(), Some(home.path()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("interrupted update files remain"));
        assert!(error.contains("nh model set <id>"));
        assert_eq!(
            selected_model_with_home(Some("explicit"), &resolver(), Some(home.path())).unwrap(),
            "explicit"
        );
    }

    #[test]
    fn malformed_oversized_and_special_preference_files_are_refused() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join(".nosis")).unwrap();
        let path = preference_path(home.path());

        fs::write(&path, "two\nlines\n").unwrap();
        assert!(read_preference(home.path())
            .unwrap_err()
            .to_string()
            .contains("malformed"));
        fs::write(&path, "x".repeat(MAX_MODEL_BYTES + 1)).unwrap();
        assert!(read_preference(home.path())
            .unwrap_err()
            .to_string()
            .contains("exceeds"));
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(read_preference(home.path())
            .unwrap_err()
            .to_string()
            .contains("regular file"));
    }

    #[test]
    fn saved_model_file_contains_only_route_id() {
        let home = tempfile::tempdir().unwrap();
        set_with_home(home.path(), "saved").unwrap();

        assert_eq!(
            fs::read_to_string(preference_path(home.path())).unwrap(),
            "saved\n"
        );
        assert_eq!(fs::read_dir(home.path().join(".nosis")).unwrap().count(), 1);
    }

    #[test]
    fn interrupted_preference_replacement_restores_previous_value() {
        let home = tempfile::tempdir().unwrap();
        let directory = inspect_preference_dir(home.path(), true).unwrap().unwrap();
        let path = preference_path(home.path());
        fs::write(&path, "saved\n").unwrap();

        let error =
            publish_preference_with(&directory, &path, Some("saved\n"), "explicit\n", || {
                Err(io::Error::new(io::ErrorKind::Interrupted, "test stop"))
            })
            .unwrap_err();

        assert!(error.to_string().contains("previous value restored"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "saved\n");
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    }

    #[test]
    fn concurrent_preference_is_not_overwritten_and_restore_stage_is_retained() {
        let home = tempfile::tempdir().unwrap();
        let directory = inspect_preference_dir(home.path(), true).unwrap().unwrap();
        let path = preference_path(home.path());
        fs::write(&path, "saved\n").unwrap();

        let error =
            publish_preference_with(&directory, &path, Some("saved\n"), "explicit\n", || {
                fs::write(&path, "competitor\n")
            })
            .unwrap_err();

        assert!(error.to_string().contains("recovery bytes remain"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "competitor\n");
        let recovery = fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.contains(".model.nh-restore-"))
            })
            .expect("retained recovery stage");
        assert_eq!(fs::read_to_string(recovery).unwrap(), "saved\n");
    }
}
