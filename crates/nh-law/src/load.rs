//! Layered law loading, guarded source reads, constitution assembly, and policy compilation.

use crate::model::{Autonomy, ConstitutionSources, Law, LoadOptions, Policy};
use crate::{
    AGENTS_LABEL, BUNDLED_LAW, MAX_CONSTITUTION_BYTES, MEMORY_LABEL, OPERATING_LAW_LABEL,
    PROJECT_LAW_LABEL, REPO_RESTRICTION_WARNING, SECTION_JOINER, USER_LAW_LABEL,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::unix::fs::OpenOptionsExt as _;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt as _;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
compile_error!("guarded reads require a verified no-follow open flag for this target");

#[cfg(windows)]
const NO_FOLLOW_OPEN_FLAG: u32 = 0x0020_0000;
#[cfg(target_os = "linux")]
const NO_FOLLOW_OPEN_FLAG: i32 = 0x2_0000;
#[cfg(target_os = "macos")]
const NO_FOLLOW_OPEN_FLAG: i32 = 0x0100;

const STAT_REFUSAL: &str = "could not stat";
const READ_REFUSAL: &str = "could not read";
const UTF8_REFUSAL: &str = "not valid UTF-8";
const ZERO_BYTE_REFUSAL: &str = "file was non-empty at stat but returned zero bytes";

/// Result of a bounded, no-follow read of one configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardedRead {
    /// The complete file contents within the requested byte cap.
    Text(String),
    /// The path did not exist when it was inspected.
    Absent,
    /// The read was refused for the contained, secret-free reason.
    Refused(String),
}

#[derive(Clone, Copy)]
enum ParseFailure {
    Warn,
    Reject,
}

/// Load every law source. Missing files are optional; malformed or unreadable files
/// become warnings and the remaining safe defaults stay active.
pub fn load(repo_root: &Path, opts: &LoadOptions) -> Law {
    let home = home_dir();
    load_with_home(repo_root, opts, home.as_deref())
}

/// Load every law source and reject invalid law files.
pub fn load_checked(repo_root: &Path, opts: &LoadOptions) -> anyhow::Result<Law> {
    let home = home_dir();
    load_checked_with_home(repo_root, opts, home.as_deref())
}

/// Assemble a deterministic constitution with fixed labels, order, and separators.
pub fn assemble_constitution(sources: &ConstitutionSources) -> String {
    let mut sections = Vec::with_capacity(5);

    push_section(
        &mut sections,
        OPERATING_LAW_LABEL,
        sources.bundled.as_deref(),
    );
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

pub(super) fn load_with_home(repo_root: &Path, opts: &LoadOptions, home: Option<&Path>) -> Law {
    load_with_home_mode(repo_root, opts, home, ParseFailure::Warn)
        .expect("warning mode handles invalid law files")
}

pub(super) fn load_checked_with_home(
    repo_root: &Path,
    opts: &LoadOptions,
    home: Option<&Path>,
) -> anyhow::Result<Law> {
    load_with_home_mode(repo_root, opts, home, ParseFailure::Reject)
}

fn load_with_home_mode(
    repo_root: &Path,
    opts: &LoadOptions,
    home: Option<&Path>,
    parse_failure: ParseFailure,
) -> anyhow::Result<Law> {
    let mut warnings = Vec::new();

    // SECURITY INVARIANT: malformed bundled policy is never treated as valid. Warning mode
    // omits it and records a warning; checked mode rejects the entire load.
    let bundled = match parse_law(BUNDLED_LAW) {
        Ok(law) => Some(law),
        Err(error) => match parse_failure {
            ParseFailure::Warn => {
                warnings.push("bundled law is malformed - safe defaults kept".to_owned());
                None
            }
            ParseFailure::Reject => return Err(invalid_law_error("bundled_law.toml", error)),
        },
    };

    let user = if let Some(home) = home {
        read_optional_law(
            &home.join(".nosis").join("law.toml"),
            None,
            "user law",
            &mut warnings,
            parse_failure,
        )?
    } else {
        warnings.push("home directory not found - user law skipped".to_owned());
        None
    };

    let repo_law_path = repo_root.join(".nosis").join("law.toml");
    let repo = read_optional_law(
        &repo_law_path,
        Some(repo_root),
        "repo .nosis/law.toml",
        &mut warnings,
        parse_failure,
    )?;
    if repo.as_ref().is_some_and(repo_tries_to_weaken) {
        warnings.push(REPO_RESTRICTION_WARNING.to_owned());
    }

    let agents_md = read_guarded_text(
        &repo_root.join("AGENTS.md"),
        Some(repo_root),
        "project AGENTS.md",
        &mut warnings,
    );
    let memory = read_guarded_text(
        &repo_root.join(".nosis").join("memory.md"),
        Some(repo_root),
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
        bundled: constitution_text(bundled.as_ref()),
        user_law_text: constitution_text(user.as_ref()),
        repo_law_text: constitution_text(repo.as_ref()),
        agents_md,
        memory,
    };

    Ok(Law {
        constitution: assemble_constitution(&sources),
        policy,
        warnings,
    })
}

pub(super) fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    const HOME_ENV: &str = "USERPROFILE";
    #[cfg(not(windows))]
    const HOME_ENV: &str = "HOME";

    std::env::var_os(HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The user's home directory for locating user-global `~/.nosis/*` config, resolved the same way
/// user law is (`USERPROFILE` on Windows, `HOME` elsewhere). None if unset.
pub fn user_home_dir() -> Option<PathBuf> {
    home_dir()
}

fn read_optional_law(
    path: &Path,
    contain_under: Option<&Path>,
    label: &str,
    warnings: &mut Vec<String>,
    parse_failure: ParseFailure,
) -> anyhow::Result<Option<LawFile>> {
    let Some(text) = read_guarded_text(path, contain_under, label, warnings) else {
        return Ok(None);
    };
    match parse_law(&text) {
        Ok(law) => Ok(Some(law)),
        Err(error) => match parse_failure {
            ParseFailure::Warn => {
                warnings.push(format!("could not parse {label} - defaults kept"));
                Ok(None)
            }
            ParseFailure::Reject => Err(invalid_law_error(&path.display().to_string(), error)),
        },
    }
}

/// SECURITY INVARIANT: law loaders admit optional external source text only through
/// `read_guarded`; refused sources are omitted and recorded as secret-free warnings.
pub(super) fn read_guarded_text(
    path: &Path,
    contain_under: Option<&Path>,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<String> {
    match read_guarded(path, contain_under, MAX_CONSTITUTION_BYTES) {
        GuardedRead::Text(text) => Some(text),
        GuardedRead::Absent => None,
        GuardedRead::Refused(reason) => {
            warnings.push(guarded_warning(label, &reason));
            None
        }
    }
}

fn guarded_warning(label: &str, reason: &str) -> String {
    match reason {
        STAT_REFUSAL => format!("could not stat {label} - source skipped"),
        READ_REFUSAL => format!("could not read {label} - source skipped"),
        reason if reason.starts_with("exceeds ") => {
            format!("refused {label}: {reason} - skipped")
        }
        _ => format!("refused {label}: {reason}"),
    }
}

/// Read one regular file without following its final path component.
///
/// When `contain_under` is present, the resolved path must remain below that
/// root. The metadata size and the bytes actually read are both capped.
pub fn read_guarded(path: &Path, contain_under: Option<&Path>, max_bytes: usize) -> GuardedRead {
    read_guarded_with_before_open(path, contain_under, max_bytes, || {})
}

pub(super) fn read_guarded_with_before_open(
    path: &Path,
    contain_under: Option<&Path>,
    max_bytes: usize,
    before_open: impl FnOnce(),
) -> GuardedRead {
    // SECURITY INVARIANT: the pre-open check rejects a symlink final component and every
    // non-regular file; `open_no_follow` below prevents a substituted final symlink from being
    // followed.
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return GuardedRead::Absent,
        Err(_) => return GuardedRead::Refused(STAT_REFUSAL.to_owned()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return GuardedRead::Refused("not a regular file (symlink or special)".to_owned());
    }

    if let Some(root) = contain_under {
        // SECURITY INVARIANT: this pre-open containment check canonicalizes both operands
        // and rejects paths that already resolve outside the requested root.
        let canonical_path = match fs::canonicalize(path) {
            Ok(path) => path,
            Err(_) => return GuardedRead::Refused("cannot canonicalize".to_owned()),
        };
        let canonical_root = match fs::canonicalize(root) {
            Ok(root) => root,
            Err(_) => return GuardedRead::Refused("cannot canonicalize".to_owned()),
        };
        if !canonical_path.starts_with(canonical_root) {
            return GuardedRead::Refused("resolves outside the repository".to_owned());
        }
    }

    let max_bytes_u64 = u64::try_from(max_bytes).unwrap_or(u64::MAX);
    if metadata.len() > max_bytes_u64 {
        return GuardedRead::Refused(format!("exceeds {max_bytes} bytes"));
    }

    before_open();
    let file = match open_no_follow(path) {
        Ok(file) => file,
        Err(_) => return GuardedRead::Refused(READ_REFUSAL.to_owned()),
    };
    let capacity = usize::try_from(metadata.len()).unwrap_or(max_bytes);
    let mut bytes = Vec::with_capacity(capacity);
    if file
        .take(max_bytes_u64.saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return GuardedRead::Refused(READ_REFUSAL.to_owned());
    }
    if bytes.len() > max_bytes {
        return GuardedRead::Refused(format!("exceeds {max_bytes} bytes"));
    }
    // On Windows, OPEN_REPARSE_POINT opens a swapped-in link itself and a read
    // returns zero bytes. A prior non-empty stat makes that a refusal, not text.
    if metadata.len() > 0 && bytes.is_empty() {
        return GuardedRead::Refused(ZERO_BYTE_REFUSAL.to_owned());
    }
    match String::from_utf8(bytes) {
        Ok(text) => GuardedRead::Text(text),
        Err(_) => GuardedRead::Refused(UTF8_REFUSAL.to_owned()),
    }
}

/// SECURITY INVARIANT: the target-specific open flag refuses to follow a symlink in the final
/// path component, including one substituted after metadata and containment checks.
fn open_no_follow(path: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true).custom_flags(NO_FOLLOW_OPEN_FLAG);
    options.open(path)
}

pub(super) fn parse_law(text: &str) -> anyhow::Result<LawFile> {
    Ok(toml::from_str(text)?)
}

fn invalid_law_error(source: &str, error: anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!("invalid law file {source}: {error}")
}

pub(super) fn constitution_text(law: Option<&LawFile>) -> Option<String> {
    law.and_then(|law| law.constitution.as_ref())
        .and_then(|section| section.text.clone())
}

pub(super) fn autonomy_from(
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

pub(super) fn repo_tries_to_weaken(law: &LawFile) -> bool {
    law.autonomy
        .as_ref()
        .and_then(|rule| rule.default.as_deref())
        .is_some_and(|value| value != "ask")
        || law
            .write
            .as_ref()
            .is_some_and(|write| write.auto.as_ref().is_some_and(|paths| !paths.is_empty()))
        || law.credential.as_ref().is_some_and(|credentials| {
            credentials.values().any(|rule| {
                rule.audience
                    .as_ref()
                    .is_some_and(|audience| !audience.is_empty())
            })
        })
}

/// SECURITY INVARIANT: `load_with_home_mode` derives autonomy without repository input, and this
/// compiler admits repository law only to ask and block lists. A cloned repository can add
/// restrictions but cannot remove them or grant itself auto-approval or a credential audience.
pub(super) fn compile_policy(
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
        read_block: Vec::new(),
        send_block: Vec::new(),
        credential_audiences: BTreeMap::new(),
        exec_block: Vec::new(),
    };

    for law in [bundled, user].into_iter().flatten() {
        if let Some(write) = &law.write {
            extend_unique(&mut policy.write_auto, write.auto.as_deref());
        }
        if let Some(credentials) = &law.credential {
            for (entry, rule) in credentials {
                extend_unique(
                    policy
                        .credential_audiences
                        .entry(entry.clone())
                        .or_default(),
                    rule.audience.as_deref(),
                );
            }
        }
    }
    for law in [bundled, user, repo].into_iter().flatten() {
        if let Some(write) = &law.write {
            extend_unique(&mut policy.write_ask, write.ask.as_deref());
            extend_unique(&mut policy.write_block, write.block.as_deref());
        }
        if let Some(read) = &law.read {
            extend_unique(&mut policy.read_block, read.block.as_deref());
        }
        if let Some(send) = &law.send {
            extend_unique(&mut policy.send_block, send.block.as_deref());
        }
        if let Some(exec) = &law.exec {
            extend_unique(&mut policy.exec_block, exec.block.as_deref());
        }
    }

    policy
}

pub(super) fn extend_unique(target: &mut Vec<String>, source: Option<&[String]>) {
    for value in source.into_iter().flatten() {
        if !target.contains(value) {
            target.push(value.clone());
        }
    }
}

pub(super) fn push_section(sections: &mut Vec<String>, label: &str, content: Option<&str>) {
    let Some(content) = content else {
        return;
    };
    if content.trim().is_empty() {
        return;
    }

    sections.push(format!("{label}\n\n{}", content.trim_end()));
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LawFile {
    pub(super) constitution: Option<ConstitutionSection>,
    pub(super) write: Option<WriteRules>,
    pub(super) read: Option<ReadRules>,
    pub(super) send: Option<SendRules>,
    pub(super) credential: Option<BTreeMap<String, CredentialRule>>,
    pub(super) exec: Option<ExecRules>,
    pub(super) autonomy: Option<AutonomyRule>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConstitutionSection {
    pub(super) text: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WriteRules {
    pub(super) auto: Option<Vec<String>>,
    pub(super) ask: Option<Vec<String>>,
    pub(super) block: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadRules {
    pub(super) block: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SendRules {
    pub(super) block: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CredentialRule {
    pub(super) audience: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ExecRules {
    // Parsed for law-file compatibility; execution already asks by default.
    #[serde(rename = "ask")]
    pub(super) _ask: Option<Vec<String>>,
    pub(super) block: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AutonomyRule {
    pub(super) default: Option<String>,
}
