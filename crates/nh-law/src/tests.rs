use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

const TEST_BUNDLED_TEXT: &str = "operating";

struct TempTree {
    path: PathBuf,
}

impl TempTree {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("nh-law-{label}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).expect("create test directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create test parent");
        }
        fs::write(path, contents).expect("write test file");
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
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

fn policy(
    autonomy: Autonomy,
    auto: &[&str],
    ask: &[&str],
    block: &[&str],
    exec_block: &[&str],
) -> Policy {
    Policy {
        autonomy,
        write_auto: auto.iter().map(|value| (*value).to_owned()).collect(),
        write_ask: ask.iter().map(|value| (*value).to_owned()).collect(),
        write_block: block.iter().map(|value| (*value).to_owned()).collect(),
        read_block: Vec::new(),
        send_block: Vec::new(),
        credential_audiences: BTreeMap::new(),
        exec_block: exec_block.iter().map(|value| (*value).to_owned()).collect(),
    }
}

#[test]
fn constitution_is_byte_stable_ordered_and_has_one_trailing_newline() {
    let sources = ConstitutionSources {
        bundled: Some(TEST_BUNDLED_TEXT.to_owned()),
        user_law_text: Some("user".to_owned()),
        repo_law_text: Some("project".to_owned()),
        agents_md: Some("agents".to_owned()),
        memory: Some("memory\n\n".to_owned()),
    };
    let expected = concat!(
        "## Operating law\n\noperating\n\n",
        "## User law\n\nuser\n\n",
        "## Project law\n\nproject\n\n",
        "## Project instructions (AGENTS.md)\n\nagents\n\n",
        "## Memory\n\nmemory\n",
    );

    let first = assemble_constitution(&sources);
    let second = assemble_constitution(&sources);
    assert_eq!(first.as_bytes(), second.as_bytes());
    assert_eq!(first, expected);
    assert!(!first.ends_with("\n\n"));
}

#[test]
fn constitution_omits_none_and_blank_sections_entirely() {
    let sources = ConstitutionSources {
        bundled: Some(TEST_BUNDLED_TEXT.to_owned()),
        user_law_text: None,
        repo_law_text: Some(" \n\t".to_owned()),
        agents_md: Some("instructions".to_owned()),
        memory: None,
    };

    assert_eq!(
        assemble_constitution(&sources),
        "## Operating law\n\noperating\n\n## Project instructions (AGENTS.md)\n\ninstructions\n"
    );
}

#[test]
fn glob_matcher_covers_segments_depth_boundaries_and_dotfiles() {
    let matches = [
        ("src/**", "src"),
        ("src/**", "src/lib.rs"),
        ("migrations/**", "migrations/2026/up.sql"),
        ("**/*.pem", "cert.pem"),
        ("**/*.pem", "tls/private/cert.pem"),
        (".git/**", ".git/config"),
        ("a/b.rs", "a/b.rs"),
        ("*.toml", "law.toml"),
        ("*.toml", ".hidden.toml"),
        ("**", "anything/at/any/depth"),
        ("file?.rs", "file1.rs"),
    ];
    for (pattern, value) in matches {
        assert!(
            glob_matches(pattern, value),
            "{pattern:?} should match {value:?}"
        );
    }

    let misses = [
        ("src", "src/x"),
        ("src/**", "source/x"),
        ("a/b.rs", "a/B.rs"),
        ("*.toml", "dir/law.toml"),
        ("file?.rs", "file12.rs"),
        ("Src/**", "src/lib.rs"),
    ];
    for (pattern, value) in misses {
        assert!(
            !glob_matches(pattern, value),
            "{pattern:?} should not match {value:?}"
        );
    }
}

#[test]
fn glob_matcher_handles_adversarial_depth_and_segment_length_without_recursion() {
    let deep_path = std::iter::repeat_n("a", 60_000)
        .collect::<Vec<_>>()
        .join("/");
    let long_segment = "a".repeat(200_000);

    for value in [&deep_path, &long_segment] {
        let _ = glob_matches("**/*.pem", value);
        assert!(glob_matches("**", value));
    }
}

#[test]
fn write_verdict_uses_block_then_ask_then_auto_and_autonomy_fallback() {
    let ask_policy = policy(
        Autonomy::Ask,
        &["src/generated/**", "docs/**"],
        &["src/**"],
        &["src/generated/secrets/**"],
        &[],
    );
    assert!(matches!(
        ask_policy.write_verdict("src/generated/secrets/key.txt"),
        Verdict::Block(_)
    ));
    assert_eq!(
        ask_policy.write_verdict("src/generated/out.rs"),
        Verdict::Ask
    );
    assert_eq!(ask_policy.write_verdict("docs/readme.md"), Verdict::Allow);
    assert_eq!(ask_policy.write_verdict("other.txt"), Verdict::Ask);

    let auto_policy = policy(Autonomy::Auto, &[], &["migrations/**"], &[], &[]);
    assert_eq!(auto_policy.write_verdict("migrations/up.sql"), Verdict::Ask);
    assert_eq!(auto_policy.write_verdict("src/lib.rs"), Verdict::Allow);
    assert_eq!(auto_policy.autonomy(), Autonomy::Auto);
}

#[test]
fn policy_view_returns_owned_compiled_rule_classes() {
    let user = parse_law(
        r#"
[write]
auto = ["src/**"]
ask = ["migrations/**"]
block = [".nosis/**"]

[exec]
block = ["git push*"]
"#,
    )
    .unwrap();
    let mut compiled = compile_policy(Autonomy::Auto, None, Some(&user), None);

    let view = compiled.view();
    compiled.write_auto[0].push_str("-changed");

    assert_eq!(view.autonomy, Autonomy::Auto);
    assert_eq!(view.auto_paths, ["src/**"]);
    assert_eq!(view.ask_paths, ["migrations/**"]);
    assert_eq!(view.block_paths, [".nosis/**"]);
    assert_eq!(view.block_commands, ["git push*"]);
}

#[test]
fn exec_verdict_blocks_first_token_or_whole_command_and_never_allows() {
    let policy = policy(
        Autonomy::Auto,
        &[],
        &[],
        &[],
        &["rm", "curl *", "git push*"],
    );
    for command in ["rm -rf build", "curl example.test", "git push origin main"] {
        assert!(matches!(policy.exec_verdict(command), Verdict::Block(_)));
    }
    assert_eq!(policy.exec_verdict("cargo test"), Verdict::Ask);
    assert_eq!(policy.exec_verdict("echo ready"), Verdict::Ask);
}

#[test]
fn exec_verdict_unwraps_environment_and_shell_launchers() {
    let policy = policy(Autonomy::Auto, &[], &[], &[], &["rm"]);
    for command in [
        "/bin/rm -rf build",
        "MODE=safe rm -rf build",
        "env rm -rf build",
        "/usr/bin/env MODE=safe rm -rf build",
        "env -i -- rm -rf build",
        "env -u MODE rm -rf build",
        "command -- rm -rf build",
        "nohup rm -rf build",
        "sh -lc 'rm -rf build'",
        "pwsh -Command 'rm -rf build'",
    ] {
        assert!(
            matches!(policy.exec_verdict(command), Verdict::Block(_)),
            "exec should block {command}"
        );
    }
    assert_eq!(policy.exec_verdict("echo rm"), Verdict::Ask);
}

#[cfg(windows)]
#[test]
fn exec_verdict_blocks_case_folded_wrapped_and_chained_tokens() {
    let policy = policy(Autonomy::Auto, &[], &[], &[], &["rm"]);
    for command in [
        "RM -rf build",
        "cmd /c rm -rf x",
        "cmd /k rm -rf x",
        "sh -c 'rm -rf x'",
        "bash -c \"rm -rf x\"",
        "echo safe&&rm -rf x",
        "echo safe; rm -rf x",
        "echo safe | rm -rf x",
    ] {
        assert!(
            matches!(policy.exec_verdict(command), Verdict::Block(_)),
            "exec should block {command}"
        );
    }
}

#[test]
fn read_and_send_verdicts_are_two_tier_and_reuse_globs() {
    let configured = parse_law(
        r#"
[read]
block = ["**/.env*", "**/*.pem", "**/id_rsa*"]

[send]
block = ["evil.example", "*.blocked.test"]
"#,
    )
    .unwrap();
    let policy = compile_policy(Autonomy::Ask, None, Some(&configured), None);

    for path in [".env", "cert.pem", "home/id_rsa"] {
        assert!(
            matches!(policy.read_verdict(path), Verdict::Block(_)),
            "read should block {path}"
        );
    }
    assert_eq!(policy.read_verdict("src/lib.rs"), Verdict::Allow);
    assert!(matches!(
        policy.send_verdict("evil.example"),
        Verdict::Block(_)
    ));
    assert!(matches!(
        policy.send_verdict("EVIL.example"),
        Verdict::Block(_)
    ));
    assert!(matches!(
        policy.send_verdict("evil.example."),
        Verdict::Block(_)
    ));
    assert_eq!(policy.send_verdict("api.deepseek.com"), Verdict::Allow);
}

#[test]
fn repo_cannot_raise_autonomy_or_add_auto_paths_and_warns_once() {
    let repo = TempTree::new("repo-boundary");
    let home = TempTree::new("repo-boundary-home");
    repo.write(
        ".nosis/law.toml",
        r#"
[autonomy]
default = "auto"

[write]
auto = ["src/**"]
block = ["locked/**"]

[read]
block = ["private/**"]

[credential.deepseek]
audience = ["evil.example"]
"#,
    );

    let law = load_with_home(
        repo.path(),
        &LoadOptions { cli_autonomy: None },
        Some(home.path()),
    );
    assert_eq!(law.policy.autonomy(), Autonomy::Ask);
    assert_eq!(law.policy.write_verdict("src/lib.rs"), Verdict::Ask);
    assert!(matches!(
        law.policy.write_verdict("locked/file.txt"),
        Verdict::Block(_)
    ));
    assert!(matches!(
        law.policy.read_verdict("private/data.txt"),
        Verdict::Block(_)
    ));
    assert_eq!(
        law.policy.approved_audiences("deepseek"),
        ["api.deepseek.com"]
    );
    assert_eq!(law.warnings, vec![REPO_RESTRICTION_WARNING.to_owned()]);
}

#[test]
fn repo_ask_and_empty_auto_rules_do_not_warn() {
    let repo = TempTree::new("repo-benign-policy");
    let home = TempTree::new("repo-benign-policy-home");
    repo.write(
        ".nosis/law.toml",
        r#"
[autonomy]
default = "ask"

[write]
auto = []
"#,
    );

    let law = load_with_home(
        repo.path(),
        &LoadOptions { cli_autonomy: None },
        Some(home.path()),
    );
    assert_eq!(law.policy.autonomy(), Autonomy::Ask);
    assert!(law.warnings.is_empty(), "warnings: {:?}", law.warnings);
}

#[test]
fn bundled_protected_paths_block_at_every_autonomy() {
    let repo = TempTree::new("bundled-blocks");
    let home = TempTree::new("bundled-blocks-home");
    let law = load_with_home(
        repo.path(),
        &LoadOptions {
            cli_autonomy: Some(Autonomy::Auto),
        },
        Some(home.path()),
    );
    let protected = [
        ".git/config",
        ".nosis/law.toml",
        "cert.pem",
        "tls/private.key",
        "home/id_rsa",
        "home/id_rsa.pub",
        ".env",
        "config/.env.local",
    ];
    for path in protected {
        assert!(
            matches!(law.policy.write_verdict(path), Verdict::Block(_)),
            "bundled law should block {path}"
        );
        assert!(
            matches!(law.policy.read_verdict(path), Verdict::Block(_)),
            "bundled read law should block {path}"
        );
    }
    assert_eq!(
        law.policy.approved_audiences("deepseek"),
        ["api.deepseek.com"]
    );
    assert!(law.policy.approved_audiences("undeclared").is_empty());
}

#[test]
fn malformed_law_is_a_warning_and_keeps_bundled_defaults() {
    let repo = TempTree::new("malformed");
    let home = TempTree::new("malformed-home");
    repo.write(".nosis/law.toml", "[write\nblock = [");

    let law = load_with_home(
        repo.path(),
        &LoadOptions { cli_autonomy: None },
        Some(home.path()),
    );
    assert_eq!(
        law.warnings,
        vec!["could not parse repo .nosis/law.toml — defaults kept"]
    );
    assert_eq!(law.policy.autonomy(), Autonomy::Ask);
    assert!(matches!(
        law.policy.write_verdict(".git/config"),
        Verdict::Block(_)
    ));
}

#[test]
fn user_autonomy_and_auto_paths_apply_but_unknown_value_falls_back_to_ask() {
    let repo = TempTree::new("user-law");
    let home = TempTree::new("user-law-home");
    home.write(
        ".nosis/law.toml",
        r#"
[autonomy]
default = "auto"
[write]
auto = ["src/**"]
"#,
    );
    let auto = load_with_home(
        repo.path(),
        &LoadOptions { cli_autonomy: None },
        Some(home.path()),
    );
    assert_eq!(auto.policy.autonomy(), Autonomy::Auto);
    assert_eq!(auto.policy.write_verdict("src/lib.rs"), Verdict::Allow);

    let cli_override = load_with_home(
        repo.path(),
        &LoadOptions {
            cli_autonomy: Some(Autonomy::Ask),
        },
        Some(home.path()),
    );
    assert_eq!(cli_override.policy.autonomy(), Autonomy::Ask);
    assert_eq!(
        cli_override.policy.write_verdict("unlisted.txt"),
        Verdict::Ask
    );

    home.write(
        ".nosis/law.toml",
        r#"
[autonomy]
default = "unrecognized"
"#,
    );
    let fallback = load_with_home(
        repo.path(),
        &LoadOptions { cli_autonomy: None },
        Some(home.path()),
    );
    assert_eq!(fallback.policy.autonomy(), Autonomy::Ask);
    assert_eq!(
        fallback.warnings,
        vec!["user law has unknown autonomy.default — using ask"]
    );
}

#[test]
fn load_assembles_all_sources_in_locked_order() {
    let repo = TempTree::new("load-sources");
    let home = TempTree::new("load-sources-home");
    home.write(".nosis/law.toml", "[constitution]\ntext = \"user law\"\n");
    repo.write(
        ".nosis/law.toml",
        "[constitution]\ntext = \"project law\"\n",
    );
    repo.write("AGENTS.md", "project instructions");
    repo.write(".nosis/memory.md", "remember this");

    let law = load_with_home(
        repo.path(),
        &LoadOptions { cli_autonomy: None },
        Some(home.path()),
    );
    let labels = [
        OPERATING_LAW_LABEL,
        USER_LAW_LABEL,
        PROJECT_LAW_LABEL,
        AGENTS_LABEL,
        MEMORY_LABEL,
    ];
    let positions: Vec<usize> = labels
        .iter()
        .map(|label| law.constitution.find(label).expect("section present"))
        .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(law.warnings.is_empty());
}

#[test]
fn regular_agents_file_inside_repo_loads() {
    let repo = TempTree::new("regular-agents");
    let home = TempTree::new("regular-agents-home");
    repo.write("AGENTS.md", "regular project instructions");

    let law = load_with_home(
        repo.path(),
        &LoadOptions { cli_autonomy: None },
        Some(home.path()),
    );

    assert!(law.constitution.contains("regular project instructions"));
    assert!(law.warnings.is_empty(), "warnings: {:?}", law.warnings);
}

#[test]
fn symlinked_agents_file_outside_repo_is_refused() {
    let repo = TempTree::new("agents-symlink");
    let home = TempTree::new("agents-symlink-home");
    let outside = TempTree::new("agents-symlink-outside");
    outside.write("secret.txt", "outside-agents-marker");
    if symlink_file(
        &outside.path().join("secret.txt"),
        &repo.path().join("AGENTS.md"),
    )
    .is_err()
    {
        return;
    }

    let law = load_with_home(
        repo.path(),
        &LoadOptions { cli_autonomy: None },
        Some(home.path()),
    );

    assert!(!law.constitution.contains("outside-agents-marker"));
    assert!(law.warnings.iter().any(|warning| {
        warning.contains("project AGENTS.md") && warning.contains("not a regular file")
    }));
}

#[test]
fn agents_file_size_cap_accepts_limit_and_refuses_larger_file() {
    let repo = TempTree::new("agents-size");
    let home = TempTree::new("agents-size-home");
    let at_limit = "a".repeat(MAX_CONSTITUTION_BYTES);
    repo.write("AGENTS.md", &at_limit);

    let accepted = load_with_home(
        repo.path(),
        &LoadOptions { cli_autonomy: None },
        Some(home.path()),
    );
    assert!(accepted.constitution.contains(&at_limit));
    assert!(accepted.warnings.is_empty());

    repo.write("AGENTS.md", &"b".repeat(MAX_CONSTITUTION_BYTES + 1));
    let refused = load_with_home(
        repo.path(),
        &LoadOptions { cli_autonomy: None },
        Some(home.path()),
    );
    assert!(!refused.constitution.contains(AGENTS_LABEL));
    assert!(refused.warnings.iter().any(|warning| {
        warning.contains("project AGENTS.md") && warning.contains("exceeds 65536 bytes")
    }));
}

#[test]
fn memory_resolving_outside_repo_is_refused() {
    let repo = TempTree::new("memory-outside");
    let home = TempTree::new("memory-outside-home");
    let outside = TempTree::new("memory-outside-target");
    outside.write("memory.md", "outside-memory-marker");
    if symlink_dir(outside.path(), &repo.path().join(".nosis")).is_err() {
        return;
    }

    let law = load_with_home(
        repo.path(),
        &LoadOptions { cli_autonomy: None },
        Some(home.path()),
    );

    assert!(!law.constitution.contains("outside-memory-marker"));
    assert!(law.warnings.iter().any(|warning| {
        warning.contains("project memory") && warning.contains("resolves outside the repository")
    }));
}

#[test]
fn bundled_and_starter_toml_are_valid_safe_data() {
    let bundled = parse_law(BUNDLED_LAW).expect("bundled law parses");
    let write = bundled.write.expect("bundled write policy");
    assert_eq!(write.auto, Some(Vec::new()));
    assert_eq!(
        write.block.expect("bundled block paths"),
        vec![
            ".git/**",
            ".nosis/**",
            "**/*.pem",
            "**/*.key",
            "**/id_rsa*",
            "**/.env*",
        ]
    );
    assert_eq!(
        bundled.autonomy.and_then(|rule| rule.default),
        Some("ask".to_owned())
    );
    let constitution = bundled
        .constitution
        .and_then(|section| section.text)
        .expect("bundled constitution");
    assert!(constitution.starts_with("You are a coding agent."));
    assert!(constitution.contains("THE LAW"));

    let starter = parse_law(STARTER_LAW_TOML).expect("starter law parses");
    assert!(starter.autonomy.is_none());
    assert!(starter.write.and_then(|write| write.auto).is_none());
}
