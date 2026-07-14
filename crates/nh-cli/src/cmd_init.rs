//! `nh init` - set up .nosis/ and the secret-guard pre-commit hook. Idempotent.

use std::fs;
use std::path::Path;

/// .nosis/.gitignore: runtime artifacts and auth material never reach git.
const GITIGNORE: &str = "# nosis-harness runtime artifacts - never commit\nreceipts.jsonl\n*.log\nauth*\n";

/// Starter route catalog for repos that have none, so `nh run` works right after
/// `nh init`. Catalog stays DATA: this embeds the repo-root catalog.toml at build
/// time - routes are never hard-coded in Rust.
const CATALOG_STARTER: &str = include_str!("../../../catalog.toml");

/// Pre-commit secret guard. The `{4,}` tails keep the pattern from matching its own
/// source line, so committing this repo does not trip the hook on itself.
const PRE_COMMIT: &str = "#!/bin/sh\n\
# nosis-harness secret guard - installed by `nh init`.\n\
# Blocks commits whose staged additions contain key-shaped strings (sk-/csk-/JWT).\n\
pattern='(^|[^A-Za-z0-9_])(sk-|csk-)[A-Za-z0-9_-]{4,}|eyJ[A-Za-z0-9_-]{4,}\\.[A-Za-z0-9_-]{4,}\\.[A-Za-z0-9_-]+'\n\
if git diff --cached --no-color --unified=0 | grep -E '^\\+[^+]' | grep -E -q \"$pattern\"; then\n\
  echo \"nh: commit blocked - staged changes contain a key-shaped string (sk-/csk-/JWT). Remove it; store keys with 'nh key add <entry>'.\" >&2\n\
  exit 1\n\
fi\n\
exit 0\n";

pub fn run() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    for line in init_at(&cwd)? {
        println!("{line}");
    }
    Ok(())
}

/// Create .nosis/, .nosis/.gitignore, starter catalog/law files (when absent),
/// and (when .git/ exists) the pre-commit hook.
/// Returns one confirmation line per thing created; ["already set up"] when nothing was.
/// Existing files are never overwritten. No .git directory → hook skipped silently.
pub fn init_at(root: &Path) -> anyhow::Result<Vec<String>> {
    let mut lines = Vec::new();

    let nosis = root.join(".nosis");
    if !nosis.is_dir() {
        fs::create_dir_all(&nosis)?;
        lines.push("created .nosis/".to_string());
    }

    let gitignore = nosis.join(".gitignore");
    if !gitignore.is_file() {
        fs::write(&gitignore, GITIGNORE)?;
        lines.push("created .nosis/.gitignore".to_string());
    }

    let catalog = root.join("catalog.toml");
    if !catalog.is_file() {
        fs::write(&catalog, CATALOG_STARTER)?;
        lines.push("created catalog.toml (starter routes - edit to add providers)".to_string());
    }

    let law = nosis.join("law.toml");
    if !law.is_file() {
        fs::write(&law, nh_law::STARTER_LAW_TOML)?;
        lines.push("created .nosis/law.toml (starter policy)".to_string());
    }

    let git_dir = root.join(".git");
    if git_dir.is_dir() {
        let hooks = git_dir.join("hooks");
        fs::create_dir_all(&hooks)?;
        let hook = hooks.join("pre-commit");
        if !hook.is_file() {
            fs::write(&hook, PRE_COMMIT)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&hook, fs::Permissions::from_mode(0o755))?;
            }
            lines.push("installed .git/hooks/pre-commit (blocks key-shaped strings)".to_string());
        }
    }

    if lines.is_empty() {
        lines.push("already set up".to_string());
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_nosis_gitignore_and_catalog_then_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let lines = init_at(tmp.path()).unwrap();

        assert!(tmp.path().join(".nosis").is_dir());
        let gi = fs::read_to_string(tmp.path().join(".nosis").join(".gitignore")).unwrap();
        assert!(gi.contains("receipts.jsonl"));
        assert!(gi.contains("*.log"));
        assert!(gi.contains("auth*"));
        let law = fs::read_to_string(tmp.path().join(".nosis").join("law.toml")).unwrap();
        assert_eq!(law, nh_law::STARTER_LAW_TOML);
        // no .git in the tempdir → exactly four things created, hook skipped silently
        assert_eq!(lines.len(), 4);

        let again = init_at(tmp.path()).unwrap();
        assert_eq!(again, vec!["already set up".to_string()]);
    }

    #[test]
    fn starter_catalog_parses_and_resolves_a_route() {
        let tmp = tempfile::tempdir().unwrap();
        init_at(tmp.path()).unwrap();
        let text = fs::read_to_string(tmp.path().join("catalog.toml")).unwrap();
        // The starter catalog must be usable by `nh run` as-is (and contain no
        // banned ids - from_toml rejects those).
        let resolver = nh_routes::RouteResolver::from_toml(&text).unwrap();
        assert!(!resolver.available().is_empty());
    }

    #[test]
    fn never_overwrites_an_existing_catalog() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("catalog.toml"), "# user's own catalog\n").unwrap();
        init_at(tmp.path()).unwrap();
        let text = fs::read_to_string(tmp.path().join("catalog.toml")).unwrap();
        assert!(text.contains("user's own catalog"));
    }

    #[test]
    fn never_overwrites_an_existing_law() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join(".nosis")).unwrap();
        fs::write(tmp.path().join(".nosis").join("law.toml"), "# user's policy\n").unwrap();
        init_at(tmp.path()).unwrap();
        let text = fs::read_to_string(tmp.path().join(".nosis").join("law.toml")).unwrap();
        assert_eq!(text, "# user's policy\n");
    }

    #[test]
    fn installs_pre_commit_hook_when_git_exists() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join(".git")).unwrap();

        let lines = init_at(tmp.path()).unwrap();
        assert_eq!(lines.len(), 5);

        let hook_path = tmp.path().join(".git").join("hooks").join("pre-commit");
        let hook = fs::read_to_string(&hook_path).unwrap();
        assert!(hook.starts_with("#!/bin/sh"));
        assert!(hook.contains("git diff --cached"));
        assert!(hook.contains("exit 1"));
    }

    #[test]
    fn never_overwrites_an_existing_hook() {
        let tmp = tempfile::tempdir().unwrap();
        let hooks = tmp.path().join(".git").join("hooks");
        fs::create_dir_all(&hooks).unwrap();
        fs::write(hooks.join("pre-commit"), "#!/bin/sh\n# user's own hook\n").unwrap();

        init_at(tmp.path()).unwrap();
        let hook = fs::read_to_string(hooks.join("pre-commit")).unwrap();
        assert!(hook.contains("user's own hook"));
    }
}
