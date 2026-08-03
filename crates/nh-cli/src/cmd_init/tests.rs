use super::*;

fn git(root: &Path, args: &[&str]) -> std::process::Output {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git is available in the test environment");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn init_git(root: &Path) {
    git(root, &["init", "--quiet"]);
}

fn reported_hooks_dir(root: &Path) -> PathBuf {
    let output = git(root, &["rev-parse", "--git-path", "hooks"]);
    let path = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[test]
fn creates_nosis_gitignore_and_catalog_then_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let lines = init_at(tmp.path()).unwrap();

    assert!(tmp.path().join(".nosis").is_dir());
    let gi = fs::read_to_string(tmp.path().join(".nosis").join(".gitignore")).unwrap();
    assert!(gi.contains("receipts.jsonl"));
    assert!(gi.contains("fleet/"));
    assert!(gi.contains("sessions/"));
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
fn existing_gitignore_is_extended_without_reordering_and_second_init_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let nosis = tmp.path().join(".nosis");
    fs::create_dir(&nosis).unwrap();
    fs::write(
        nosis.join(".gitignore"),
        "# user rule\ncustom-cache/\nfleet/\n",
    )
    .unwrap();
    fs::write(tmp.path().join("catalog.toml"), CATALOG_STARTER).unwrap();
    fs::write(nosis.join("law.toml"), nh_law::STARTER_LAW_TOML).unwrap();

    let lines = init_at(tmp.path()).unwrap();
    let updated = fs::read_to_string(nosis.join(".gitignore")).unwrap();

    assert!(updated.starts_with("# user rule\ncustom-cache/\nfleet/\n"));
    assert_eq!(updated.matches("fleet/\n").count(), 1);
    for required in ["receipts.jsonl", "sessions/", "*.log", "auth*"] {
        assert_eq!(
            updated.lines().filter(|line| *line == required).count(),
            1,
            "missing or duplicated {required}: {updated}"
        );
        assert!(lines
            .iter()
            .any(|line| line == &format!("added {required} to .nosis/.gitignore")));
    }

    assert_eq!(init_at(tmp.path()).unwrap(), ["already set up"]);
    assert_eq!(
        fs::read_to_string(nosis.join(".gitignore")).unwrap(),
        updated
    );
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
    fs::write(
        tmp.path().join(".nosis").join("law.toml"),
        "# user's policy\n",
    )
    .unwrap();
    init_at(tmp.path()).unwrap();
    let text = fs::read_to_string(tmp.path().join(".nosis").join("law.toml")).unwrap();
    assert_eq!(text, "# user's policy\n");
}

#[test]
fn real_repo_installs_executable_hook_at_git_reported_path_and_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(tmp.path());
    let hook_path = reported_hooks_dir(tmp.path()).join("pre-commit");

    let lines = init_at(tmp.path()).unwrap();
    assert!(lines
        .iter()
        .any(|line| line == "installed Git pre-commit hook (blocks key-shaped strings)"));
    let hook = fs::read_to_string(&hook_path).unwrap();
    assert!(hook.starts_with("#!/bin/sh"));
    assert!(hook.contains("git diff --cached"));
    assert!(hook.contains("exit 1"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(&hook_path).unwrap().permissions().mode() & 0o111,
            0
        );
    }

    let again = init_at(tmp.path()).unwrap();
    assert_eq!(again, vec!["already set up".to_string()]);
}

#[test]
fn registered_linked_worktree_uses_git_reported_hooks_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("main");
    let linked = tmp.path().join("linked");
    fs::create_dir(&main).unwrap();
    init_git(&main);
    git(
        &main,
        &[
            "-c",
            "user.name=Nosis Test",
            "-c",
            "user.email=nosis-test@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    git(
        &main,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            linked.to_str().unwrap(),
        ],
    );
    let hook = reported_hooks_dir(&linked).join("pre-commit");

    let lines = init_at(&linked).unwrap();

    assert!(hook.is_file());
    assert!(lines
        .iter()
        .any(|line| line == "installed Git pre-commit hook (blocks key-shaped strings)"));
}

#[test]
fn never_overwrites_an_existing_hook() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(tmp.path());
    let hooks = reported_hooks_dir(tmp.path());
    fs::create_dir_all(&hooks).unwrap();
    fs::write(hooks.join("pre-commit"), "#!/bin/sh\n# user's own hook\n").unwrap();

    let lines = init_at(tmp.path()).unwrap();
    let hook = fs::read_to_string(hooks.join("pre-commit")).unwrap();
    assert!(hook.contains("user's own hook"));
    assert!(lines.iter().any(|line| line == EXISTING_HOOK_WARNING));
}

#[test]
fn pre_commit_guard_uses_the_runtime_key_shape_alternatives_verbatim() {
    let hook = pre_commit();
    assert!(hook.contains(nh_vault::KEY_SHAPE_ALTERNATIVES));
}

#[test]
fn forged_gitfile_pointing_outside_does_not_install_an_external_hook() {
    let tmp = tempfile::tempdir().unwrap();
    let attacker = tmp.path().join("attacker");
    let victim = tmp.path().join("victim");
    fs::create_dir(&attacker).unwrap();
    fs::create_dir(&victim).unwrap();
    init_git(&attacker);
    fs::write(
        victim.join(".git"),
        format!("gitdir: {}\n", attacker.join(".git").display()),
    )
    .unwrap();

    let lines = init_at(&victim).unwrap();

    assert!(lines.iter().any(|line| line == HOOK_WARNING));
    assert!(!attacker
        .join(".git")
        .join("hooks")
        .join("pre-commit")
        .exists());
}

#[test]
fn symlinked_git_directory_does_not_install_an_external_hook() {
    let tmp = tempfile::tempdir().unwrap();
    let attacker = tmp.path().join("attacker");
    let victim = tmp.path().join("victim");
    fs::create_dir(&attacker).unwrap();
    fs::create_dir(&victim).unwrap();
    init_git(&attacker);
    if symlink_dir(&attacker.join(".git"), &victim.join(".git")).is_err() {
        return;
    }

    let lines = init_at(&victim).unwrap();

    assert!(lines.iter().any(|line| line == HOOK_WARNING));
    assert!(!attacker
        .join(".git")
        .join("hooks")
        .join("pre-commit")
        .exists());
}

#[test]
fn unresolvable_gitfile_reports_that_the_hook_was_not_installed() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join(".git"), "not a gitdir pointer\n").unwrap();

    let lines = init_at(tmp.path()).unwrap();
    assert!(lines.iter().any(|line| line == HOOK_WARNING));
    assert!(!lines.iter().any(|line| line == "already set up"));
}

#[test]
fn custom_hooks_path_reports_that_the_default_hook_was_not_installed() {
    let tmp = tempfile::tempdir().unwrap();
    init_git(tmp.path());
    git(tmp.path(), &["config", "core.hooksPath", "custom-hooks"]);

    let lines = init_at(tmp.path()).unwrap();
    assert!(lines.iter().any(|line| line == HOOK_WARNING));
    assert!(!tmp
        .path()
        .join(".git")
        .join("hooks")
        .join("pre-commit")
        .exists());
    assert!(!tmp.path().join("custom-hooks").join("pre-commit").exists());
}

#[test]
fn existing_hook_symlink_is_left_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let hooks = tmp.path().join("hooks");
    let target = tmp.path().join("outside-hook");
    fs::create_dir(&hooks).unwrap();
    fs::write(&target, "outside sentinel\n").unwrap();
    if symlink_file(&target, &hooks.join("pre-commit")).is_err() {
        return;
    }
    let mut lines = Vec::new();

    install_hook(&hooks, "installed", &mut lines).unwrap();

    assert_eq!(lines, vec![EXISTING_HOOK_WARNING.to_string()]);
    assert_eq!(fs::read_to_string(target).unwrap(), "outside sentinel\n");
}
