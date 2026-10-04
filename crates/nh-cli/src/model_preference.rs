//! Operator-owned, user-global default route selection.

use std::path::{Path, PathBuf};

use nh_routes::RouteResolver;
use nh_vault::Scrubber;

use crate::cmd_run;
use crate::private_state::{PrivateStateFile, PrivateStateRead};

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

#[cfg(test)]
fn preference_path(home: &Path) -> PathBuf {
    home.join(".nosis").join("model")
}

fn preference_file(home: &Path, create: bool) -> anyhow::Result<Option<PrivateStateFile>> {
    PrivateStateFile::open(home, None, "model", "model", MAX_MODEL_BYTES, create).map_err(|error| {
        anyhow::anyhow!(
            "could not safely access saved model preference at ~/.nosis/model: {error}; inspect ~/.nosis and run `nh model clear` if the saved preference should be removed"
        )
    })
}

fn read_preference(home: &Path) -> anyhow::Result<Option<String>> {
    let Some(file) = preference_file(home, false)? else {
        return Ok(None);
    };
    let preference = file.read().map_err(|error| {
        anyhow::anyhow!(
            "refused saved model preference at ~/.nosis/model: {error}; inspect the file and run `nh model clear` to remove it deliberately"
        )
    })?;
    match preference {
        PrivateStateRead::Text(text) => parse_preference(&text).map(Some),
        PrivateStateRead::Interrupted => anyhow::bail!(
            "saved model preference is absent while interrupted update files remain in ~/.nosis; run `nh model set <id>` to choose a model again"
        ),
        PrivateStateRead::Absent => Ok(None),
    }
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
    let file = preference_file(home, true)?.expect("created preference directory");
    let previous = match file.read().map_err(|error| {
        anyhow::anyhow!(
            "refused saved model preference at ~/.nosis/model before update: {error}; inspect the file and run `nh model clear` to remove it deliberately"
        )
    })? {
        PrivateStateRead::Text(text) => Some(text),
        PrivateStateRead::Absent | PrivateStateRead::Interrupted => None,
    };
    let replacement = format!("{model}\n");
    file.publish(previous.as_deref(), &replacement)
        .map_err(|error| {
            anyhow::anyhow!(
                "could not safely update saved model preference at ~/.nosis/model: {error}; inspect any reported recovery file, then retry `nh model set <id>`"
            )
        })
}

fn clear_with_home(home: &Path) -> anyhow::Result<bool> {
    let Some(file) = preference_file(home, false)? else {
        return Ok(false);
    };
    let original = match file.read().map_err(|error| {
        anyhow::anyhow!(
            "refused saved model preference at ~/.nosis/model before removal: {error}; inspect the file before retrying `nh model clear`"
        )
    })? {
        PrivateStateRead::Text(text) => text,
        PrivateStateRead::Absent | PrivateStateRead::Interrupted => return Ok(false),
    };
    file.remove(&original).map_err(|error| {
        anyhow::anyhow!(
            "could not clear saved model preference at ~/.nosis/model: {error}; inspect the file before retrying `nh model clear`"
        )
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io;

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
        let file = preference_file(home.path(), true).unwrap().unwrap();
        let directory = file.path().parent().unwrap();
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
        let error = read_preference(home.path()).unwrap_err().to_string();
        assert!(error.contains("refused saved model preference"), "{error}");
        assert!(error.contains("regular file"), "{error}");
        assert!(error.contains("nh model clear"), "{error}");
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
        let file = preference_file(home.path(), true).unwrap().unwrap();
        let directory = file.path().parent().unwrap();
        let path = preference_path(home.path());
        fs::write(&path, "saved\n").unwrap();

        let error = file
            .publish_with_test_hook(Some("saved\n"), "explicit\n", || {
                Err(io::Error::new(io::ErrorKind::Interrupted, "test stop"))
            })
            .unwrap_err();

        assert!(error.to_string().contains("previous value restored"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "saved\n");
        assert_eq!(fs::read_dir(directory).unwrap().count(), 1);
    }

    #[test]
    fn concurrent_preference_is_not_overwritten_and_restore_stage_is_retained() {
        let home = tempfile::tempdir().unwrap();
        let file = preference_file(home.path(), true).unwrap().unwrap();
        let directory = file.path().parent().unwrap();
        let path = preference_path(home.path());
        fs::write(&path, "saved\n").unwrap();

        let error = file
            .publish_with_test_hook(Some("saved\n"), "explicit\n", || {
                fs::write(&path, "competitor\n")
            })
            .unwrap_err();

        assert!(error.to_string().contains("recovery bytes remain"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "competitor\n");
        let recovery = fs::read_dir(directory)
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
