//! `nh init` - set up .nosis/ and the secret-guard pre-commit hook. Idempotent.

use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

/// .nosis/.gitignore: runtime artifacts and auth material never reach git.
const GITIGNORE: &str =
    "# nosis-harness runtime artifacts - never commit\nreceipts.jsonl\nfleet/\nsessions/\n*.log\nauth*\n";
const REQUIRED_GITIGNORE_LINES: &[&str] =
    &["receipts.jsonl", "fleet/", "sessions/", "*.log", "auth*"];

/// Starter route catalog for repos that have none, so `nh run` works right after
/// `nh init`. Catalog stays DATA: this embeds the repo-root catalog.toml at build
/// time - routes are never hard-coded in Rust.
const CATALOG_STARTER: &str = include_str!("../../../catalog.toml");

/// Pre-commit secret guard. Keep these formats congruent with nh-vault's runtime
/// scrubber. Regex syntax remains grep ERE-compatible because the installed hook
/// cannot depend on the Rust binary being available during every commit.
const PRE_COMMIT_PREFIX: &str = "#!/bin/sh\n\
# nosis-harness secret guard - installed by `nh init`.\n\
# Blocks commits whose staged additions contain a key-shaped string.\n\
pattern='(^|[^A-Za-z0-9_])(";
const PRE_COMMIT_SUFFIX: &str = ")'\n\
if git diff --cached --no-color --unified=0 | grep -E '^\\+[^+]' | grep -E -q \"$pattern\"; then\n\
  echo \"nh: commit blocked - staged changes contain a key-shaped string. Remove it; store keys with 'nh key add <entry>'.\" >&2\n\
  exit 1\n\
fi\n\
exit 0\n";
const HOOK_WARNING: &str =
    "pre-commit secret guard NOT installed (worktree/custom hooks path) - install manually";
const EXISTING_HOOK_WARNING: &str =
    "pre-commit secret guard NOT installed (existing hook preserved) - chain it manually";
const NO_WORK_TREE_WARNING: &str = "pre-commit secret guard NOT installed (not a Git work tree)";

fn pre_commit() -> String {
    let mut hook = String::with_capacity(
        PRE_COMMIT_PREFIX.len() + nh_vault::KEY_SHAPE_ALTERNATIVES.len() + PRE_COMMIT_SUFFIX.len(),
    );
    hook.push_str(PRE_COMMIT_PREFIX);
    hook.push_str(nh_vault::KEY_SHAPE_ALTERNATIVES);
    hook.push_str(PRE_COMMIT_SUFFIX);
    hook
}

pub fn run() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    for line in init_at(&cwd)? {
        println!("{line}");
    }
    Ok(())
}

/// Create .nosis/, .nosis/.gitignore, starter catalog/law files (when absent),
/// and (when Git metadata is resolvable) the pre-commit hook.
/// Returns one confirmation line per thing created. A Git work tree with no remaining work returns
/// ["already set up"]. Existing files are never overwritten.
/// No Git metadata means there is no hook to install.
pub fn init_at(root: &Path) -> anyhow::Result<Vec<String>> {
    let mut lines = Vec::new();

    let nosis = root.join(".nosis");
    if !nosis.is_dir() {
        fs::create_dir_all(&nosis)?;
        lines.push("created .nosis/".to_string());
    }

    let gitignore = nosis.join(".gitignore");
    update_gitignore(&gitignore, &mut lines)?;

    let catalog = root.join("catalog.toml");
    if !catalog.is_file() {
        fs::write(&catalog, CATALOG_STARTER)?;
        lines.push("created catalog.toml (trusted bundled routes)".to_string());
    }

    let law = nosis.join("law.toml");
    if !law.is_file() {
        fs::write(&law, nh_law::STARTER_LAW_TOML)?;
        lines.push("created .nosis/law.toml (starter policy)".to_string());
    }

    match resolve_hooks_dir(root) {
        HooksResolution::Install(hooks) => install_hook(
            &hooks,
            "installed Git pre-commit hook (blocks key-shaped strings)",
            &mut lines,
        )?,
        HooksResolution::NoWorkTree => lines.push(NO_WORK_TREE_WARNING.to_string()),
        HooksResolution::Refused => lines.push(HOOK_WARNING.to_string()),
    }

    if lines.is_empty() {
        lines.push("already set up".to_string());
    }
    Ok(lines)
}

fn update_gitignore(path: &Path, lines: &mut Vec<String>) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            anyhow::bail!(
                "refused: .nosis/.gitignore is not a regular file - replace it, then rerun `nh init`"
            )
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::write(path, GITIGNORE)?;
            lines.push("created .nosis/.gitignore".to_string());
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    }

    let existing = fs::read(path)?;
    let missing = REQUIRED_GITIGNORE_LINES
        .iter()
        .copied()
        .filter(|required| {
            !existing
                .split(|byte| *byte == b'\n')
                .any(|line| line.strip_suffix(b"\r").unwrap_or(line) == required.as_bytes())
        })
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(());
    }

    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    if !existing.is_empty() && existing.last() != Some(&b'\n') {
        file.write_all(b"\n")?;
    }
    for entry in missing {
        writeln!(file, "{entry}")?;
        lines.push(format!("added {entry} to .nosis/.gitignore"));
    }
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

enum HooksResolution {
    Install(PathBuf),
    NoWorkTree,
    Refused,
}

fn resolve_hooks_dir(root: &Path) -> HooksResolution {
    let git_marker = root.join(".git");
    let (gitfile, marker_exists) = match fs::symlink_metadata(&git_marker) {
        Ok(metadata) if metadata.file_type().is_symlink() => return HooksResolution::Refused,
        Ok(metadata) if metadata.is_file() => (true, true),
        Ok(metadata) if metadata.is_dir() => (false, true),
        Ok(_) => return HooksResolution::Refused,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (false, false),
        Err(_) => return HooksResolution::Refused,
    };

    let inside = match std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
    {
        Ok(output) => output,
        Err(_) => return HooksResolution::Refused,
    };
    if !inside.status.success() {
        return if marker_exists {
            HooksResolution::Refused
        } else {
            HooksResolution::NoWorkTree
        };
    }
    let inside = match String::from_utf8(inside.stdout) {
        Ok(value) => value,
        Err(_) => return HooksResolution::Refused,
    };
    match inside.trim() {
        "true" => {}
        "false" => return HooksResolution::NoWorkTree,
        _ => return HooksResolution::Refused,
    }

    if gitfile && !gitfile_is_registered_worktree(root) {
        return HooksResolution::Refused;
    }

    let configured = match std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["config", "--get", "core.hooksPath"])
        .output()
    {
        Ok(output) => output,
        Err(_) => return HooksResolution::Refused,
    };
    if configured.status.success() {
        let value = match String::from_utf8(configured.stdout) {
            Ok(value) => value,
            Err(_) => return HooksResolution::Refused,
        };
        if !value.trim().is_empty() {
            return HooksResolution::Refused;
        }
    } else if configured.status.code() != Some(1) {
        return HooksResolution::Refused;
    }

    let hooks = match std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--git-path", "hooks"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return HooksResolution::Refused,
    };
    let hooks = match String::from_utf8(hooks.stdout) {
        Ok(value) => value,
        Err(_) => return HooksResolution::Refused,
    };
    let hooks = hooks.trim();
    if hooks.is_empty() || hooks.contains(['\n', '\r', '\0']) {
        return HooksResolution::Refused;
    }
    let hooks = PathBuf::from(hooks);
    if hooks.is_absolute() {
        HooksResolution::Install(hooks)
    } else {
        HooksResolution::Install(root.join(hooks))
    }
}

fn gitfile_is_registered_worktree(root: &Path) -> bool {
    let listed = match std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return false,
    };
    let canonical_root = match fs::canonicalize(root) {
        Ok(root) => root,
        Err(_) => return false,
    };
    listed.stdout.split(|byte| *byte == 0).any(|field| {
        let Some(path) = field.strip_prefix(b"worktree ") else {
            return false;
        };
        let Ok(path) = std::str::from_utf8(path) else {
            return false;
        };
        fs::canonicalize(path).is_ok_and(|path| path == canonical_root)
    })
}

fn install_hook(hooks: &Path, confirmation: &str, lines: &mut Vec<String>) -> anyhow::Result<()> {
    fs::create_dir_all(hooks)?;
    let hook = hooks.join("pre-commit");
    let expected = pre_commit();
    match fs::symlink_metadata(&hook) {
        // Existing regular files, symlinks, and special entries are all left untouched.
        // An exact copy of our hook is already installed; every other entry gets an
        // actionable warning instead of silently weakening the advertised guard.
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                lines.push(EXISTING_HOOK_WARNING.to_string());
                return Ok(());
            }
            let mut existing = Vec::with_capacity(expected.len().saturating_add(1));
            let same = fs::File::open(&hook)
                .and_then(|file| {
                    file.take(expected.len() as u64 + 1)
                        .read_to_end(&mut existing)
                })
                .is_ok()
                && existing == expected.as_bytes();
            if !same {
                lines.push(EXISTING_HOOK_WARNING.to_string());
            }
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    fs::write(&hook, expected)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755))?;
    }
    lines.push(confirmation.to_string());
    Ok(())
}

#[cfg(test)]
mod tests;
