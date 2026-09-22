//! Explicit migration of byte-identical historical bundled catalogs.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};

use nh_law::{read_guarded, GuardedRead};
use nh_routes::RouteResolver;
use nh_vault::Scrubber;

use crate::cmd_run;

const CURRENT_CATALOG: &str = include_str!("../../../catalog.toml");
const CATALOG_V020: &str = include_str!("../catalog-history/v0.2.0.toml");
const CATALOG_V021: &str = include_str!("../catalog-history/v0.2.1.toml");
const MAX_CATALOG_BYTES: usize = 1024 * 1024;
const MAX_CONFIRM_BYTES: usize = 16;

#[derive(Clone, Copy)]
struct HistoricalCatalog {
    label: &'static str,
    backup_label: &'static str,
    text: &'static str,
}

const HISTORICAL_CATALOGS: &[HistoricalCatalog] = &[
    HistoricalCatalog {
        label: "v0.1.0 or v0.2.0 bundled catalog",
        backup_label: "v0.2.0",
        text: CATALOG_V020,
    },
    HistoricalCatalog {
        label: "v0.2.1 bundled catalog",
        backup_label: "v0.2.1",
        text: CATALOG_V021,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Yes,
    No,
    Cancel,
}

trait CatalogUi {
    fn line(&mut self, line: &str) -> io::Result<()>;
    fn confirm(&mut self, prompt: &str) -> io::Result<Answer>;
}

struct ConsoleUi {
    scrubber: Scrubber,
}

impl ConsoleUi {
    fn new() -> Self {
        Self {
            scrubber: Scrubber::new(Vec::new()),
        }
    }

    fn safe(&self, value: &str) -> String {
        cmd_run::safe_line(&self.scrubber, value)
    }
}

impl CatalogUi for ConsoleUi {
    fn line(&mut self, line: &str) -> io::Result<()> {
        println!("{}", self.safe(line));
        Ok(())
    }

    fn confirm(&mut self, prompt: &str) -> io::Result<Answer> {
        {
            let mut stdout = io::stdout().lock();
            write!(stdout, "{}", self.safe(prompt))?;
            stdout.flush()?;
        }
        read_answer(&mut io::stdin().lock())
    }
}

pub(crate) fn migrate() -> anyhow::Result<()> {
    require_interactive(io::stdin().is_terminal(), io::stdout().is_terminal())?;
    let cwd = std::env::current_dir()?;
    let path = locate_catalog(&cwd)?;
    let mut ui = ConsoleUi::new();
    migrate_with(&path, &mut ui, || Ok(()))
}

fn require_interactive(stdin: bool, stdout: bool) -> anyhow::Result<()> {
    if stdin && stdout {
        Ok(())
    } else {
        anyhow::bail!("catalog migration requires an interactive terminal for review and consent")
    }
}

fn locate_catalog(start: &Path) -> anyhow::Result<PathBuf> {
    for root in start.ancestors() {
        let path = root.join("catalog.toml");
        match fs::symlink_metadata(&path) {
            Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_file() => {
                return Ok(path)
            }
            Ok(_) => anyhow::bail!(
                "refused catalog migration: catalog.toml must be a regular file; symlinks and special files are not accepted"
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => anyhow::bail!("could not inspect catalog.toml: {error}"),
        }
    }
    anyhow::bail!("no catalog.toml found - run `nh init` to create one")
}

fn migrate_with(
    path: &Path,
    ui: &mut dyn CatalogUi,
    after_remove: impl FnOnce() -> io::Result<()>,
) -> anyhow::Result<()> {
    let existing = read_catalog(path)?;
    if existing == CURRENT_CATALOG {
        anyhow::bail!("catalog.toml already matches this nh version")
    }
    let historical = HISTORICAL_CATALOGS
        .iter()
        .find(|historical| catalogs_match_across_line_endings(historical.text, &existing))
        .copied()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "catalog.toml is custom or changed - it was not modified; review and update it manually"
            )
        })?;

    let resolved = fs::canonicalize(path)?;
    let backup_path = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("catalog path has no parent"))?
        .join(format!(
            "catalog.toml.nh-backup-{}",
            historical.backup_label
        ));
    ui.line(&format!("Catalog to replace: {}", resolved.display()))?;
    for line in migration_preview(historical)? {
        ui.line(&line)?;
    }
    ui.line(&format!("Original backup: {}", backup_path.display()))?;
    ui.line("No key, policy, approval, budget, or saved model setting will be changed.")?;
    ui.line(
        "Keep other nh instances closed: catalog.toml is briefly absent during no-clobber publication.",
    )?;

    match ui.confirm("Replace this known historical catalog? [y/N] ")? {
        Answer::Yes => {}
        Answer::No => {
            ui.line("Catalog migration declined. No files changed.")?;
            return Ok(());
        }
        Answer::Cancel => {
            ui.line("Catalog migration cancelled. No files changed.")?;
            return Ok(());
        }
    }

    let backup = publish_migration_with(path, historical, &existing, after_remove)?;
    ui.line("Catalog migration complete.")?;
    ui.line(&format!(
        "The original catalog remains at {}.",
        backup
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("the versioned backup path")
    ))?;
    Ok(())
}

fn catalogs_match_across_line_endings(left: &str, right: &str) -> bool {
    left.replace("\r\n", "\n") == right.replace("\r\n", "\n")
}

fn read_catalog(path: &Path) -> anyhow::Result<String> {
    let root = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("catalog path has no parent"))?;
    match read_guarded(path, Some(root), MAX_CATALOG_BYTES) {
        GuardedRead::Text(text) => Ok(text),
        GuardedRead::Absent => anyhow::bail!("catalog.toml disappeared before migration"),
        GuardedRead::Refused(reason) => anyhow::bail!("refused catalog.toml: {reason}"),
    }
}

fn migration_preview(historical: HistoricalCatalog) -> anyhow::Result<Vec<String>> {
    let old = RouteResolver::from_toml(historical.text)?;
    let new = RouteResolver::from_toml(CURRENT_CATALOG)?;
    let mut lines = vec![
        format!("Recognized: {}.", historical.label),
        "Proposed target: the catalog bundled with this nh binary.".to_owned(),
        "Trust-relevant changes:".to_owned(),
    ];

    let ids = old
        .available()
        .into_iter()
        .chain(new.available())
        .collect::<BTreeSet<_>>();
    let mut change_count = 0usize;
    for id in ids {
        match (old.resolve(&id), new.resolve(&id)) {
            (Ok(_), Err(_)) => {
                lines.push(format!("- {id}: removed"));
                change_count += 1;
            }
            (Err(_), Ok(_)) => {
                lines.push(format!("- {id}: added"));
                change_count += 1;
            }
            (Ok(before), Ok(after)) => {
                let mut fields = Vec::new();
                changed(&mut fields, "provider", before.provider(), after.provider());
                changed(
                    &mut fields,
                    "provider model",
                    before.model_id(),
                    after.model_id(),
                );
                changed(&mut fields, "endpoint", before.base_url(), after.base_url());
                changed(
                    &mut fields,
                    "credential entry",
                    before.vault_entry(),
                    after.vault_entry(),
                );
                if before.wire() != after.wire() {
                    fields.push("wire protocol changed".to_owned());
                }
                if before.class() != after.class() {
                    fields.push(format!(
                        "class {} -> {}",
                        before.class().as_str(),
                        after.class().as_str()
                    ));
                }
                if before.modality() != after.modality() {
                    fields.push(format!(
                        "input capability {:?} -> {:?}",
                        before.modality(),
                        after.modality()
                    ));
                }
                if before.context() != after.context() || before.max_out() != after.max_out() {
                    fields.push(format!(
                        "limits context {:?} -> {:?}, output {:?} -> {:?}",
                        before.context(),
                        after.context(),
                        before.max_out(),
                        after.max_out()
                    ));
                }
                if before.thinking_dialect() != after.thinking_dialect()
                    || before.preserve_reasoning() != after.preserve_reasoning()
                    || before.preserve_when_thinking() != after.preserve_when_thinking()
                    || before.quirks() != after.quirks()
                {
                    fields.push("thinking or wire behavior changed".to_owned());
                }
                if format!("{:?}", before.price()) != format!("{:?}", after.price()) {
                    fields.push("pricing schedule changed".to_owned());
                }
                if !fields.is_empty() {
                    lines.push(format!("- {id}: {}", fields.join("; ")));
                    change_count += 1;
                }
            }
            (Err(_), Err(_)) => unreachable!("id came from at least one resolver"),
        }
    }
    if change_count == 0 {
        lines.push("- no route fields changed".to_owned());
    }

    lines.push("Credential destinations after migration:".to_owned());
    let destinations = new
        .available()
        .into_iter()
        .map(|id| new.resolve(&id))
        .collect::<anyhow::Result<Vec<_>>>()?
        .into_iter()
        .map(|route| (route.vault_entry().to_owned(), route.base_url().to_owned()))
        .collect::<BTreeSet<_>>();
    for (entry, endpoint) in destinations {
        lines.push(format!("- {entry} -> {endpoint}"));
    }
    Ok(lines)
}

fn changed(lines: &mut Vec<String>, label: &str, before: &str, after: &str) {
    if before != after {
        lines.push(format!("{label} {before} -> {after}"));
    }
}

#[derive(Clone)]
struct FileSnapshot {
    len: u64,
    modified: Option<std::time::SystemTime>,
    readonly: bool,
    #[cfg(unix)]
    mode: u32,
}

impl FileSnapshot {
    fn read(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "catalog is not a regular file",
            ));
        }
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt as _;
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            readonly: metadata.permissions().readonly(),
            #[cfg(unix)]
            mode: metadata.mode(),
        })
    }

    fn same_as(&self, other: &Self) -> bool {
        self.len == other.len
            && self.modified == other.modified
            && self.readonly == other.readonly
            && {
                #[cfg(unix)]
                {
                    self.mode == other.mode
                }
                #[cfg(not(unix))]
                {
                    true
                }
            }
    }
}

fn publish_migration_with(
    catalog: &Path,
    historical: HistoricalCatalog,
    original: &str,
    after_remove: impl FnOnce() -> io::Result<()>,
) -> anyhow::Result<PathBuf> {
    publish_migration_with_hooks(catalog, historical, original, || Ok(()), after_remove)
}

fn publish_migration_with_hooks(
    catalog: &Path,
    historical: HistoricalCatalog,
    original: &str,
    before_recheck: impl FnOnce() -> io::Result<()>,
    after_remove: impl FnOnce() -> io::Result<()>,
) -> anyhow::Result<PathBuf> {
    let directory = catalog
        .parent()
        .ok_or_else(|| anyhow::anyhow!("catalog path has no parent"))?;
    let backup = directory.join(format!(
        "catalog.toml.nh-backup-{}",
        historical.backup_label
    ));
    match fs::symlink_metadata(&backup) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Ok(_) => anyhow::bail!(
            "backup {} already exists - no files changed; move it only after reviewing it",
            backup.display()
        ),
        Err(error) => anyhow::bail!("could not inspect backup path: {error}"),
    }

    let initial = FileSnapshot::read(catalog)?;
    let permissions = fs::metadata(catalog)?.permissions();
    let mut stages = Vec::new();
    let backup_stage = stage_file(directory, "backup", original, &permissions)?;
    stages.push(backup_stage.clone());
    let replacement_stage =
        match stage_file(directory, "replacement", CURRENT_CATALOG, &permissions) {
            Ok(path) => path,
            Err(error) => return Err(cleanup_stages(error.into(), &stages)),
        };
    stages.push(replacement_stage.clone());
    let restore_stage = match stage_file(directory, "restore", original, &permissions) {
        Ok(path) => path,
        Err(error) => return Err(cleanup_stages(error.into(), &stages)),
    };
    stages.push(restore_stage.clone());

    if let Err(error) = fs::hard_link(&backup_stage, &backup) {
        return Err(cleanup_stages(
            anyhow::anyhow!(
                "could not publish the no-clobber catalog backup: {error}; check permissions, free space, and filesystem hard-link support"
            ),
            &stages,
        ));
    }

    if let Err(error) = before_recheck() {
        return Err(cleanup_stages(
            anyhow::anyhow!(
                "catalog migration stopped before the final source check: {error}; the catalog was not replaced and the versioned backup remains"
            ),
            &stages,
        ));
    }

    let current = match read_catalog(catalog) {
        Ok(current) => current,
        Err(error) => return Err(cleanup_stages(error, &stages)),
    };
    let current_snapshot = match FileSnapshot::read(catalog) {
        Ok(snapshot) => snapshot,
        Err(error) => return Err(cleanup_stages(error.into(), &stages)),
    };
    if current != original || !initial.same_as(&current_snapshot) {
        return Err(cleanup_stages(
            anyhow::anyhow!(
                "catalog.toml changed during migration; it was not replaced and the historical bytes remain in the versioned backup"
            ),
            &stages,
        ));
    }

    if let Err(error) = fs::remove_file(catalog) {
        return Err(cleanup_stages(
            anyhow::anyhow!("could not remove the rechecked historical catalog: {error}"),
            &stages,
        ));
    }

    if let Err(error) = after_remove() {
        let rollback = no_clobber_restore(&restore_stage, catalog);
        return Err(cleanup_stages(
            interrupted_error(error, rollback, &backup),
            &stages,
        ));
    }

    if let Err(error) = fs::hard_link(&replacement_stage, catalog) {
        let rollback = no_clobber_restore(&restore_stage, catalog);
        return Err(cleanup_stages(
            publication_error(error, rollback, &backup),
            &stages,
        ));
    }

    let cleanup_failures = remove_stages(&stages);
    if !cleanup_failures.is_empty() {
        anyhow::bail!(
            "catalog was migrated and backed up, but staging cleanup failed: {}",
            cleanup_failures.join("; ")
        )
    }
    Ok(backup)
}

fn stage_file(
    directory: &Path,
    purpose: &str,
    contents: &str,
    permissions: &fs::Permissions,
) -> io::Result<PathBuf> {
    for attempt in 0..64u8 {
        let path = directory.join(format!(
            ".catalog.toml.nh-{purpose}-{}-{attempt}.tmp",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(error) = (|| {
                    file.write_all(contents.as_bytes())?;
                    file.sync_all()?;
                    fs::set_permissions(&path, permissions.clone())?;
                    Ok::<(), io::Error>(())
                })() {
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
        "could not allocate a unique migration staging file",
    ))
}

fn no_clobber_restore(stage: &Path, catalog: &Path) -> io::Result<()> {
    fs::hard_link(stage, catalog)
}

fn interrupted_error(error: io::Error, rollback: io::Result<()>, backup: &Path) -> anyhow::Error {
    match rollback {
        Ok(()) => anyhow::anyhow!(
            "catalog migration stopped before publication: {error}; the historical catalog was restored and its backup remains at {}",
            backup.display()
        ),
        Err(rollback) => anyhow::anyhow!(
            "catalog migration stopped before publication: {error}; no-clobber restore failed: {rollback}; preserve the current catalog path and recover from {}",
            backup.display()
        ),
    }
}

fn publication_error(error: io::Error, rollback: io::Result<()>, backup: &Path) -> anyhow::Error {
    match rollback {
        Ok(()) => anyhow::anyhow!(
            "could not publish the replacement catalog without clobbering: {error}; the historical catalog was restored and its backup remains at {}",
            backup.display()
        ),
        Err(rollback) => anyhow::anyhow!(
            "could not publish the replacement catalog without clobbering: {error}; no-clobber restore also failed: {rollback}; do not overwrite the catalog path and recover from {}",
            backup.display()
        ),
    }
}

fn cleanup_stages(mut primary: anyhow::Error, stages: &[PathBuf]) -> anyhow::Error {
    let failures = remove_stages(stages);
    if !failures.is_empty() {
        primary = primary.context(format!(
            "migration staging cleanup also failed: {}",
            failures.join("; ")
        ));
    }
    primary
}

fn remove_stages(stages: &[PathBuf]) -> Vec<String> {
    let mut failures = Vec::new();
    for stage in stages {
        if let Err(error) = fs::remove_file(stage) {
            if error.kind() != io::ErrorKind::NotFound {
                failures.push(error.to_string());
            }
        }
    }
    failures
}

fn read_answer(reader: &mut dyn io::BufRead) -> io::Result<Answer> {
    let mut bytes = Vec::new();
    let mut too_long = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if bytes.is_empty() {
                Ok(Answer::Cancel)
            } else {
                break;
            };
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let content_len = newline.unwrap_or(available.len());
        let remaining = (MAX_CONFIRM_BYTES + 1).saturating_sub(bytes.len());
        let copied = content_len.min(remaining);
        bytes.extend_from_slice(&available[..copied]);
        too_long |= copied < content_len || bytes.len() > MAX_CONFIRM_BYTES;
        let consumed = newline.map_or(available.len(), |index| index + 1);
        let complete = newline.is_some();
        reader.consume(consumed);
        if complete {
            break;
        }
    }
    if too_long {
        return Ok(Answer::Cancel);
    }
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "input is not valid UTF-8"))?
        .trim_matches(|character: char| character.is_whitespace() || character == '\u{feff}');
    Ok(match value {
        "y" | "Y" | "yes" | "Yes" | "YES" => Answer::Yes,
        "" | "n" | "N" | "no" | "No" | "NO" => Answer::No,
        _ => Answer::Cancel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct TestUi {
        answer: Answer,
        output: String,
        prompts: usize,
    }

    impl TestUi {
        fn new(answer: Answer) -> Self {
            Self {
                answer,
                output: String::new(),
                prompts: 0,
            }
        }
    }

    impl CatalogUi for TestUi {
        fn line(&mut self, line: &str) -> io::Result<()> {
            self.output.push_str(line);
            self.output.push('\n');
            Ok(())
        }

        fn confirm(&mut self, prompt: &str) -> io::Result<Answer> {
            self.output.push_str(prompt);
            self.prompts += 1;
            Ok(self.answer)
        }
    }

    #[test]
    fn known_historical_catalog_requires_consent_and_preserves_original_on_decline() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("catalog.toml");
        fs::write(&path, CATALOG_V020).unwrap();
        let mut ui = TestUi::new(Answer::No);
        let called = Cell::new(false);

        migrate_with(&path, &mut ui, || {
            called.set(true);
            Ok(())
        })
        .unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), CATALOG_V020);
        assert!(!called.get());
        assert_eq!(ui.prompts, 1);
        assert!(ui.output.contains("No files changed"));
        assert!(!root.path().join("catalog.toml.nh-backup-v0.2.0").exists());
    }

    #[test]
    fn custom_catalog_is_refused_before_prompt_or_write() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("catalog.toml");
        fs::write(&path, "# custom operator catalog\n").unwrap();
        let mut ui = TestUi::new(Answer::Yes);

        let error = migrate_with(&path, &mut ui, || Ok(())).unwrap_err();

        assert!(error.to_string().contains("custom or changed"));
        assert_eq!(ui.prompts, 0);
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "# custom operator catalog\n"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn consent_installs_current_catalog_and_keeps_versioned_backup() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("catalog.toml");
        fs::write(&path, CATALOG_V021).unwrap();
        let mut ui = TestUi::new(Answer::Yes);

        migrate_with(&path, &mut ui, || Ok(())).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), CURRENT_CATALOG);
        assert_eq!(
            fs::read_to_string(root.path().join("catalog.toml.nh-backup-v0.2.1")).unwrap(),
            CATALOG_V021
        );
        assert!(ui
            .output
            .contains("Credential destinations after migration"));
        assert!(ui.output.contains(&path.display().to_string()));
        assert!(ui.output.contains("briefly absent"));
        assert!(ui
            .output
            .contains("provider model deepseek-v4-flash -> deepseek-flash"));
        assert!(ui.output.contains("input capability"));
        assert!(ui.output.contains("Catalog migration complete"));
    }

    #[test]
    fn crlf_historical_catalog_is_recognized_and_backed_up_byte_for_byte() {
        assert!(!CATALOG_V021.contains('\r'));
        let historical_crlf = CATALOG_V021.replace('\n', "\r\n");
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("catalog.toml");
        fs::write(&path, &historical_crlf).unwrap();
        let mut ui = TestUi::new(Answer::Yes);

        migrate_with(&path, &mut ui, || Ok(())).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), CURRENT_CATALOG);
        assert_eq!(
            fs::read_to_string(root.path().join("catalog.toml.nh-backup-v0.2.1")).unwrap(),
            historical_crlf
        );
    }

    #[test]
    fn interruption_after_removal_restores_without_clobbering() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("catalog.toml");
        fs::write(&path, CATALOG_V021).unwrap();
        let mut ui = TestUi::new(Answer::Yes);

        let error = migrate_with(&path, &mut ui, || {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "test interruption",
            ))
        })
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("historical catalog was restored"));
        assert_eq!(fs::read_to_string(&path).unwrap(), CATALOG_V021);
        assert_eq!(
            fs::read_to_string(root.path().join("catalog.toml.nh-backup-v0.2.1")).unwrap(),
            CATALOG_V021
        );
    }

    #[test]
    fn concurrent_catalog_wins_and_no_clobber_restore_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("catalog.toml");
        fs::write(&path, CATALOG_V021).unwrap();
        let mut ui = TestUi::new(Answer::Yes);

        let error =
            migrate_with(&path, &mut ui, || fs::write(&path, "competitor content\n")).unwrap_err();

        assert!(error
            .to_string()
            .contains("do not overwrite the catalog path"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "competitor content\n");
        assert_eq!(
            fs::read_to_string(root.path().join("catalog.toml.nh-backup-v0.2.1")).unwrap(),
            CATALOG_V021
        );
    }

    #[test]
    fn source_change_before_removal_is_detected_and_preserved() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("catalog.toml");
        fs::write(&path, CATALOG_V021).unwrap();
        let historical = HISTORICAL_CATALOGS[1];

        let error = publish_migration_with_hooks(
            &path,
            historical,
            CATALOG_V021,
            || fs::write(&path, "concurrent edit before removal\n"),
            || Ok(()),
        )
        .unwrap_err();

        assert!(error.to_string().contains("changed during migration"));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "concurrent edit before removal\n"
        );
        assert_eq!(
            fs::read_to_string(root.path().join("catalog.toml.nh-backup-v0.2.1")).unwrap(),
            CATALOG_V021
        );
    }

    #[test]
    fn existing_backup_is_never_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("catalog.toml");
        let backup = root.path().join("catalog.toml.nh-backup-v0.2.1");
        fs::write(&path, CATALOG_V021).unwrap();
        fs::write(&backup, "keep backup\n").unwrap();
        let mut ui = TestUi::new(Answer::Yes);

        let error = migrate_with(&path, &mut ui, || Ok(())).unwrap_err();

        assert!(error.to_string().contains("already exists"));
        assert_eq!(fs::read_to_string(&path).unwrap(), CATALOG_V021);
        assert_eq!(fs::read_to_string(&backup).unwrap(), "keep backup\n");
    }

    #[test]
    fn interactive_answer_is_bounded_and_default_deny() {
        assert_eq!(
            read_answer(&mut io::Cursor::new("y\n")).unwrap(),
            Answer::Yes
        );
        assert_eq!(read_answer(&mut io::Cursor::new("\n")).unwrap(), Answer::No);
        assert_eq!(
            read_answer(&mut io::Cursor::new("x".repeat(32))).unwrap(),
            Answer::Cancel
        );
        assert_eq!(
            read_answer(&mut io::Cursor::new(Vec::<u8>::new())).unwrap(),
            Answer::Cancel
        );
    }
}
