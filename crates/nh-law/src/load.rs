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

/// Load every law source. Missing files are optional; malformed or unreadable files
/// become warnings and the remaining safe defaults stay active.
pub fn load(repo_root: &Path, opts: &LoadOptions) -> Law {
    let home = home_dir();
    load_with_home(repo_root, opts, home.as_deref())
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
            None,
            "user law",
            &mut warnings,
        )
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
    );
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

    Law {
        constitution: assemble_constitution(&sources),
        policy,
        warnings,
    }
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

pub(super) fn read_optional_law(
    path: &Path,
    contain_under: Option<&Path>,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<LawFile> {
    let text = read_guarded_text(path, contain_under, label, warnings)?;
    match parse_law(&text) {
        Ok(law) => Some(law),
        Err(_) => {
            warnings.push(format!("could not parse {label} - defaults kept"));
            None
        }
    }
}

pub(super) fn read_guarded_text(
    path: &Path,
    contain_under: Option<&Path>,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => {
            warnings.push(format!("could not stat {label} - source skipped"));
            return None;
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        warnings.push(format!(
            "refused {label}: not a regular file (symlink or special)"
        ));
        return None;
    }

    if let Some(root) = contain_under {
        let canonical_path = match fs::canonicalize(path) {
            Ok(path) => path,
            Err(_) => {
                warnings.push(format!("refused {label}: cannot canonicalize"));
                return None;
            }
        };
        let canonical_root = match fs::canonicalize(root) {
            Ok(root) => root,
            Err(_) => {
                warnings.push(format!("refused {label}: cannot canonicalize"));
                return None;
            }
        };
        if !canonical_path.starts_with(canonical_root) {
            warnings.push(format!("refused {label}: resolves outside the repository"));
            return None;
        }
    }

    if metadata.len() > MAX_CONSTITUTION_BYTES as u64 {
        warnings.push(format!(
            "refused {label}: exceeds {MAX_CONSTITUTION_BYTES} bytes - skipped"
        ));
        return None;
    }

    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(_) => {
            warnings.push(format!("could not read {label} - source skipped"));
            return None;
        }
    };
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    if file
        .take(MAX_CONSTITUTION_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        warnings.push(format!("could not read {label} - source skipped"));
        return None;
    }
    if bytes.len() > MAX_CONSTITUTION_BYTES {
        warnings.push(format!(
            "refused {label}: exceeds {MAX_CONSTITUTION_BYTES} bytes - skipped"
        ));
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

pub(super) fn parse_law(text: &str) -> anyhow::Result<LawFile> {
    Ok(toml::from_str(text)?)
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
pub(super) struct ConstitutionSection {
    pub(super) text: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct WriteRules {
    pub(super) auto: Option<Vec<String>>,
    pub(super) ask: Option<Vec<String>>,
    pub(super) block: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct ReadRules {
    pub(super) block: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct SendRules {
    pub(super) block: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct CredentialRule {
    pub(super) audience: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct ExecRules {
    // Parsed for law-file compatibility; execution already asks by default.
    #[serde(rename = "ask")]
    pub(super) _ask: Option<Vec<String>>,
    pub(super) block: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct AutonomyRule {
    pub(super) default: Option<String>,
}
