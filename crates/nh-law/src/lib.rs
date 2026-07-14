//! Constitution assembly and immutable session policy for Nosis Harness.
//!
//! Policy is data: bundled, user-global, and repository law files are merged once
//! at session start. Repository law may add protections, never weaken them.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

const SECTION_JOINER: &str = "\n\n";
const OPERATING_LAW_LABEL: &str = "## Operating law";
const USER_LAW_LABEL: &str = "## User law";
const PROJECT_LAW_LABEL: &str = "## Project law";
const AGENTS_LABEL: &str = "## Project instructions (AGENTS.md)";
const MEMORY_LABEL: &str = "## Memory";
const REPO_RESTRICTION_WARNING: &str =
    "repo .nosis/law.toml cannot raise autonomy or auto-approve paths - ignored";

/// The bundled default constitution and policy.
pub const BUNDLED_LAW: &str = include_str!("bundled_law.toml");

/// Safe project-law starter written by `nh init`.
pub const STARTER_LAW_TOML: &str = include_str!("starter_law.toml");

/// Session autonomy. Repository law cannot set this value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Autonomy {
    #[default]
    Ask,
    Auto,
}

/// Effective decision for one access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Ask,
    Block(String),
}

/// Compiled, immutable policy for one session.
#[derive(Debug, Clone)]
pub struct Policy {
    autonomy: Autonomy,
    write_auto: Vec<String>,
    write_ask: Vec<String>,
    write_block: Vec<String>,
    exec_block: Vec<String>,
}

/// Constitution, policy, and non-fatal source warnings for one session.
#[derive(Debug, Clone)]
pub struct Law {
    pub constitution: String,
    pub policy: Policy,
    pub warnings: Vec<String>,
}

/// Caller-controlled load options.
#[derive(Debug, Clone, Copy)]
pub struct LoadOptions {
    pub cli_autonomy: Option<Autonomy>,
}

/// Inputs to deterministic constitution assembly.
#[derive(Debug, Clone)]
pub struct ConstitutionSources {
    pub bundled: &'static str,
    pub user_law_text: Option<String>,
    pub repo_law_text: Option<String>,
    pub agents_md: Option<String>,
    pub memory: Option<String>,
}

impl Policy {
    /// Decide whether a normalized, forward-slashed relative path may be written.
    pub fn write_verdict(&self, rel_path: &str) -> Verdict {
        if let Some(pattern) = first_match(&self.write_block, rel_path) {
            return Verdict::Block(format!(
                "protected path ({pattern}) - held even at max autonomy"
            ));
        }
        if first_match(&self.write_ask, rel_path).is_some() {
            return Verdict::Ask;
        }
        if first_match(&self.write_auto, rel_path).is_some() {
            return Verdict::Allow;
        }
        match self.autonomy {
            Autonomy::Ask => Verdict::Ask,
            Autonomy::Auto => Verdict::Allow,
        }
    }

    /// Decide whether a shell command is blocked. Execution is never auto-allowed.
    pub fn exec_verdict(&self, command: &str) -> Verdict {
        if let Some(pattern) = self
            .exec_block
            .iter()
            .find(|pattern| exec_pattern_matches(pattern, command))
        {
            return Verdict::Block(format!("blocked command ({pattern})"));
        }

        Verdict::Ask
    }

    pub fn autonomy(&self) -> Autonomy {
        self.autonomy
    }
}

/// Load every law source. Missing files are optional; malformed or unreadable files
/// become warnings and the remaining safe defaults stay active.
pub fn load(repo_root: &Path, opts: &LoadOptions) -> Law {
    let home = home_dir();
    load_with_home(repo_root, opts, home.as_deref())
}

/// Assemble a deterministic constitution with fixed labels, order, and separators.
pub fn assemble_constitution(sources: &ConstitutionSources) -> String {
    let bundled_text = parse_law(sources.bundled)
        .ok()
        .and_then(|law| law.constitution.and_then(|section| section.text));
    let mut sections = Vec::with_capacity(5);

    push_section(&mut sections, OPERATING_LAW_LABEL, bundled_text.as_deref());
    push_section(
        &mut sections,
        USER_LAW_LABEL,
        sources.user_law_text.as_deref(),
    );
    push_section(
        &mut sections,
        PROJECT_LAW_LABEL,
        sources.repo_law_text.as_deref(),
    );
    push_section(&mut sections, AGENTS_LABEL, sources.agents_md.as_deref());
    push_section(&mut sections, MEMORY_LABEL, sources.memory.as_deref());

    if sections.is_empty() {
        String::new()
    } else {
        let mut assembled = sections.join(SECTION_JOINER);
        assembled.push('\n');
        assembled
    }
}

fn load_with_home(repo_root: &Path, opts: &LoadOptions, home: Option<&Path>) -> Law {
    let mut warnings = Vec::new();

    let bundled = match parse_law(BUNDLED_LAW) {
        Ok(law) => Some(law),
        Err(_) => {
            warnings.push("bundled law is malformed - safe defaults kept".to_owned());
            None
        }
    };

    let user = if let Some(home) = home {
        read_optional_law(
            &home.join(".nosis").join("law.toml"),
            "user law",
            &mut warnings,
        )
    } else {
        warnings.push("home directory not found - user law skipped".to_owned());
        None
    };

    let repo_law_path = repo_root.join(".nosis").join("law.toml");
    let repo = read_optional_law(&repo_law_path, "repo .nosis/law.toml", &mut warnings);
    if repo.as_ref().is_some_and(repo_tries_to_weaken) {
        warnings.push(REPO_RESTRICTION_WARNING.to_owned());
    }

    let agents_md = read_optional_text(
        &repo_root.join("AGENTS.md"),
        "project AGENTS.md",
        &mut warnings,
    );
    let memory = read_optional_text(
        &repo_root.join(".nosis").join("memory.md"),
        "project memory",
        &mut warnings,
    );

    let bundled_autonomy =
        autonomy_from(bundled.as_ref(), "bundled law", &mut warnings).unwrap_or_default();
    let user_autonomy = autonomy_from(user.as_ref(), "user law", &mut warnings);
    let autonomy = opts
        .cli_autonomy
        .or(user_autonomy)
        .unwrap_or(bundled_autonomy);

    let policy = compile_policy(autonomy, bundled.as_ref(), user.as_ref(), repo.as_ref());
    let sources = ConstitutionSources {
        bundled: BUNDLED_LAW,
        user_law_text: constitution_text(user.as_ref()),
        repo_law_text: constitution_text(repo.as_ref()),
        agents_md,
        memory,
    };

    Law {
        constitution: assemble_constitution(&sources),
        policy,
        warnings,
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    const HOME_ENV: &str = "USERPROFILE";
    #[cfg(not(windows))]
    const HOME_ENV: &str = "HOME";

    std::env::var_os(HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn read_optional_law(path: &Path, label: &str, warnings: &mut Vec<String>) -> Option<LawFile> {
    let text = read_optional_text(path, label, warnings)?;
    match parse_law(&text) {
        Ok(law) => Some(law),
        Err(_) => {
            warnings.push(format!("could not parse {label} - defaults kept"));
            None
        }
    }
}

fn read_optional_text(path: &Path, label: &str, warnings: &mut Vec<String>) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => {
            warnings.push(format!("could not read {label} - source skipped"));
            None
        }
    }
}

fn parse_law(text: &str) -> anyhow::Result<LawFile> {
    Ok(toml::from_str(text)?)
}

fn constitution_text(law: Option<&LawFile>) -> Option<String> {
    law.and_then(|law| law.constitution.as_ref())
        .and_then(|section| section.text.clone())
}

fn autonomy_from(
    law: Option<&LawFile>,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<Autonomy> {
    let value = law?.autonomy.as_ref()?.default.as_deref()?;
    match value {
        "ask" => Some(Autonomy::Ask),
        "auto" => Some(Autonomy::Auto),
        _ => {
            warnings.push(format!("{label} has unknown autonomy.default - using ask"));
            Some(Autonomy::Ask)
        }
    }
}

fn repo_tries_to_weaken(law: &LawFile) -> bool {
    law.autonomy.is_some() || law.write.as_ref().is_some_and(|write| write.auto.is_some())
}

fn compile_policy(
    autonomy: Autonomy,
    bundled: Option<&LawFile>,
    user: Option<&LawFile>,
    repo: Option<&LawFile>,
) -> Policy {
    let mut policy = Policy {
        autonomy,
        write_auto: Vec::new(),
        write_ask: Vec::new(),
        write_block: Vec::new(),
        exec_block: Vec::new(),
    };

    for law in [bundled, user].into_iter().flatten() {
        if let Some(write) = &law.write {
            extend_unique(&mut policy.write_auto, write.auto.as_deref());
        }
    }
    for law in [bundled, user, repo].into_iter().flatten() {
        if let Some(write) = &law.write {
            extend_unique(&mut policy.write_ask, write.ask.as_deref());
            extend_unique(&mut policy.write_block, write.block.as_deref());
        }
        if let Some(exec) = &law.exec {
            extend_unique(&mut policy.exec_block, exec.block.as_deref());
        }
    }

    policy
}

fn extend_unique(target: &mut Vec<String>, source: Option<&[String]>) {
    for value in source.into_iter().flatten() {
        if !target.contains(value) {
            target.push(value.clone());
        }
    }
}

fn push_section(sections: &mut Vec<String>, label: &str, content: Option<&str>) {
    let Some(content) = content else {
        return;
    };
    if content.trim().is_empty() {
        return;
    }

    sections.push(format!("{label}\n\n{}", content.trim_end()));
}

fn first_match<'a>(patterns: &'a [String], value: &str) -> Option<&'a str> {
    patterns
        .iter()
        .find(|pattern| glob_matches(pattern, value))
        .map(String::as_str)
}

/// Keep the exec first-token/whole-command rule in this one function.
fn exec_pattern_matches(pattern: &str, command: &str) -> bool {
    if pattern.contains('/') {
        return glob_matches(pattern, command);
    }

    let first_token = command.split_whitespace().next().unwrap_or("");
    glob_matches(pattern, first_token) || glob_matches(pattern, command)
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    let pattern_segments: Vec<&str> = pattern.split('/').collect();
    let value_segments: Vec<&str> = value.split('/').collect();
    let mut memo = vec![vec![None; value_segments.len() + 1]; pattern_segments.len() + 1];

    fn matches_from(
        pattern: &[&str],
        value: &[&str],
        pattern_index: usize,
        value_index: usize,
        memo: &mut [Vec<Option<bool>>],
    ) -> bool {
        if let Some(result) = memo[pattern_index][value_index] {
            return result;
        }

        let result = if pattern_index == pattern.len() {
            value_index == value.len()
        } else if pattern[pattern_index] == "**" {
            matches_from(pattern, value, pattern_index + 1, value_index, memo)
                || (value_index < value.len()
                    && matches_from(pattern, value, pattern_index, value_index + 1, memo))
        } else {
            value_index < value.len()
                && segment_matches(pattern[pattern_index], value[value_index])
                && matches_from(pattern, value, pattern_index + 1, value_index + 1, memo)
        };

        memo[pattern_index][value_index] = Some(result);
        result
    }

    matches_from(&pattern_segments, &value_segments, 0, 0, &mut memo)
}

fn segment_matches(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let mut memo = vec![vec![None; value.len() + 1]; pattern.len() + 1];

    fn matches_from(
        pattern: &[char],
        value: &[char],
        pattern_index: usize,
        value_index: usize,
        memo: &mut [Vec<Option<bool>>],
    ) -> bool {
        if let Some(result) = memo[pattern_index][value_index] {
            return result;
        }

        let result = match pattern.get(pattern_index) {
            None => value_index == value.len(),
            Some('*') => {
                matches_from(pattern, value, pattern_index + 1, value_index, memo)
                    || (value_index < value.len()
                        && matches_from(pattern, value, pattern_index, value_index + 1, memo))
            }
            Some('?') => {
                value_index < value.len()
                    && matches_from(pattern, value, pattern_index + 1, value_index + 1, memo)
            }
            Some(expected) => {
                value.get(value_index) == Some(expected)
                    && matches_from(pattern, value, pattern_index + 1, value_index + 1, memo)
            }
        };

        memo[pattern_index][value_index] = Some(result);
        result
    }

    matches_from(&pattern, &value, 0, 0, &mut memo)
}

#[derive(Debug, Default, Deserialize)]
struct LawFile {
    constitution: Option<ConstitutionSection>,
    write: Option<WriteRules>,
    exec: Option<ExecRules>,
    autonomy: Option<AutonomyRule>,
}

#[derive(Debug, Default, Deserialize)]
struct ConstitutionSection {
    text: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct WriteRules {
    auto: Option<Vec<String>>,
    ask: Option<Vec<String>>,
    block: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct ExecRules {
    // Parsed for law-file compatibility; execution already asks by default.
    #[allow(dead_code)]
    ask: Option<Vec<String>>,
    block: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct AutonomyRule {
    default: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    const TEST_BUNDLED: &str = r#"
[constitution]
text = "operating"
"#;

    struct TempTree {
        path: PathBuf,
    }

    impl TempTree {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("nh-law-{label}-{}-{unique}", std::process::id()));
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
            exec_block: exec_block.iter().map(|value| (*value).to_owned()).collect(),
        }
    }

    #[test]
    fn constitution_is_byte_stable_ordered_and_has_one_trailing_newline() {
        let sources = ConstitutionSources {
            bundled: TEST_BUNDLED,
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
            bundled: TEST_BUNDLED,
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
        assert_eq!(law.warnings, vec![REPO_RESTRICTION_WARNING.to_owned()]);
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
        }
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
            vec!["could not parse repo .nosis/law.toml - defaults kept"]
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
            vec!["user law has unknown autonomy.default - using ask"]
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
}
