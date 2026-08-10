//! `nh doctor` - report install facts and actionable fixes without requiring prior setup.

use std::collections::{BTreeMap, BTreeSet};
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};

use nh_routes::{RouteClass, RouteResolver};
use nh_vault::{KeyringVault, Scrubber, Vault as _};

use crate::cmd_run;

const NETWORK_VARS: [&str; 5] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProbeValue<T> {
    Value(T),
    Unavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PathStatus {
    Running(PathBuf),
    Different { found: PathBuf, running: PathBuf },
    Missing { directory: PathBuf },
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyFact {
    entry: String,
    providers: Vec<String>,
    stored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyStatus {
    Entries(Vec<KeyFact>),
    StoreUnavailable(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CatalogStatus {
    Ready {
        location: PathBuf,
        route_count: usize,
        provider_count: usize,
        keys: KeyStatus,
    },
    Unreadable {
        location: Option<PathBuf>,
        reason: String,
    },
}

/// Only Windows has a console code page, so only Windows constructs these.
/// The type stays available everywhere so the facts struct and its tests do
/// not need their own platform split.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodePageStatus {
    Utf8,
    Other(u32),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NetworkFact {
    name: &'static str,
    set: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LocationStatus {
    Exists(PathBuf),
    Missing(PathBuf),
    Unknown {
        path: Option<PathBuf>,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DoctorFacts {
    version: String,
    binary: ProbeValue<PathBuf>,
    path: PathStatus,
    catalog: CatalogStatus,
    stdout_terminal: bool,
    no_color: bool,
    code_page: Option<CodePageStatus>,
    network: Vec<NetworkFact>,
    user_config: LocationStatus,
    repository_config: LocationStatus,
}

pub fn run() -> anyhow::Result<()> {
    let facts = probe();
    let scrubber = Scrubber::new(Vec::new());
    for line in render(&facts) {
        println!("{}", cmd_run::safe_line(&scrubber, &line));
    }
    Ok(())
}

fn probe() -> DoctorFacts {
    let binary = match std::env::current_exe() {
        Ok(path) => ProbeValue::Value(path),
        Err(error) => ProbeValue::Unavailable(error.to_string()),
    };
    let path = probe_path(&binary);
    let cwd = match std::env::current_dir() {
        Ok(path) => ProbeValue::Value(path),
        Err(error) => ProbeValue::Unavailable(error.to_string()),
    };
    let catalog = probe_catalog(&cwd);
    let user_config = nh_law::user_home_dir().map_or_else(
        || LocationStatus::Unknown {
            path: None,
            reason: "user home directory is not set".to_owned(),
        },
        |home| probe_location(home.join(".nosis")),
    );
    let repository_config = match &cwd {
        ProbeValue::Value(cwd) => probe_location(cwd.join(".nosis")),
        ProbeValue::Unavailable(reason) => LocationStatus::Unknown {
            path: None,
            reason: format!("current directory is unavailable ({reason})"),
        },
    };

    #[cfg(windows)]
    let code_page = Some(probe_code_page());
    #[cfg(not(windows))]
    let code_page = None;

    DoctorFacts {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        binary,
        path,
        catalog,
        stdout_terminal: std::io::stdout().is_terminal(),
        no_color: std::env::var_os("NO_COLOR").is_some(),
        code_page,
        network: NETWORK_VARS
            .into_iter()
            .map(|name| NetworkFact {
                name,
                set: environment_is_set(name),
            })
            .collect(),
        user_config,
        repository_config,
    }
}

fn probe_path(binary: &ProbeValue<PathBuf>) -> PathStatus {
    let running = match binary {
        ProbeValue::Value(running) => running,
        ProbeValue::Unavailable(reason) => {
            return PathStatus::Unknown(format!("running binary is unavailable ({reason})"));
        }
    };
    let Some(found) = first_nh_on_path() else {
        return PathStatus::Missing {
            directory: running
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
        };
    };
    if paths_name_same_file(&found, running) {
        PathStatus::Running(found)
    } else {
        PathStatus::Different {
            found,
            running: running.clone(),
        }
    }
}

fn first_nh_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let file_name = format!("nh{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&path)
        .map(|directory| directory.join(&file_name))
        .find(|candidate| candidate.is_file())
}

fn paths_name_same_file(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn probe_catalog(cwd: &ProbeValue<PathBuf>) -> CatalogStatus {
    let cwd = match cwd {
        ProbeValue::Value(cwd) => cwd,
        ProbeValue::Unavailable(reason) => {
            return CatalogStatus::Unreadable {
                location: None,
                reason: format!("current directory is unavailable ({reason})"),
            };
        }
    };
    let (root, catalog) = match cmd_run::find_catalog(cwd) {
        Ok(catalog) => catalog,
        Err(error) => {
            return CatalogStatus::Unreadable {
                location: None,
                reason: error.to_string(),
            }
        }
    };
    let location = root.join("catalog.toml");
    let resolver = match RouteResolver::from_toml(&catalog) {
        Ok(resolver) => resolver,
        Err(error) => {
            return CatalogStatus::Unreadable {
                location: Some(location),
                reason: error.to_string(),
            }
        }
    };
    let routes = resolver.available();
    let route_count = routes.len();
    let provider_count = resolver.available_by_provider().len();
    let mut entries: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for id in routes {
        let route = match resolver.resolve(&id) {
            Ok(route) => route,
            Err(error) => {
                return CatalogStatus::Unreadable {
                    location: Some(location),
                    reason: format!("could not resolve route {id} ({error})"),
                }
            }
        };
        if route.class() == RouteClass::Api {
            entries
                .entry(route.vault_entry().to_owned())
                .or_default()
                .insert(route.provider().to_owned());
        }
    }

    CatalogStatus::Ready {
        location,
        route_count,
        provider_count,
        keys: probe_keys(entries),
    }
}

fn probe_keys(entries: BTreeMap<String, BTreeSet<String>>) -> KeyStatus {
    let mut facts = Vec::with_capacity(entries.len());
    for (entry, providers) in entries {
        let stored = match KeyringVault.get(&entry) {
            Ok(secret) => {
                drop(secret);
                true
            }
            Err(error) if key_is_absent(&error, &entry) => false,
            Err(error) => return KeyStatus::StoreUnavailable(error.to_string()),
        };
        facts.push(KeyFact {
            entry,
            providers: providers.into_iter().collect(),
            stored,
        });
    }
    KeyStatus::Entries(facts)
}

fn key_is_absent(error: &anyhow::Error, entry: &str) -> bool {
    error
        .to_string()
        .starts_with(&format!("no key stored for \"{entry}\""))
}

fn environment_is_set(name: &str) -> bool {
    if std::env::var_os(name).is_some() {
        return true;
    }
    // Unix tools commonly use the lowercase spelling as well. `cfg!` keeps one
    // compiled path on every platform, so there is no platform-only branch here
    // that a single-platform gate run would never check.
    cfg!(unix) && std::env::var_os(name.to_ascii_lowercase()).is_some()
}

fn probe_location(path: PathBuf) -> LocationStatus {
    match path.try_exists() {
        Ok(true) => LocationStatus::Exists(path),
        Ok(false) => LocationStatus::Missing(path),
        Err(error) => LocationStatus::Unknown {
            path: Some(path),
            reason: error.to_string(),
        },
    }
}

#[cfg(windows)]
fn probe_code_page() -> CodePageStatus {
    let output = match std::process::Command::new("chcp.com").output() {
        Ok(output) if output.status.success() => output,
        _ => return CodePageStatus::Unknown,
    };
    match trailing_ascii_integer(&output.stdout) {
        Some(65001) => CodePageStatus::Utf8,
        Some(value) => CodePageStatus::Other(value),
        None => CodePageStatus::Unknown,
    }
}

#[cfg(windows)]
fn trailing_ascii_integer(bytes: &[u8]) -> Option<u32> {
    let end = bytes.iter().rposition(u8::is_ascii_digit)? + 1;
    let start = bytes[..end]
        .iter()
        .rposition(|byte| !byte.is_ascii_digit())
        .map_or(0, |index| index + 1);
    std::str::from_utf8(&bytes[start..end]).ok()?.parse().ok()
}

pub(crate) fn render(facts: &DoctorFacts) -> Vec<String> {
    let mut lines = vec!["facts:".to_owned(), format!("  version: {}", facts.version)];
    match &facts.binary {
        ProbeValue::Value(path) => lines.push(format!("  binary: {}", path.display())),
        ProbeValue::Unavailable(reason) => {
            lines.push(format!("  binary: unknown ({reason})"));
        }
    }
    match &facts.path {
        PathStatus::Running(path) => {
            lines.push(format!("  PATH: found running binary ({})", path.display()));
        }
        PathStatus::Different { found, running } => lines.push(format!(
            "  PATH: found different nh ({}) than running binary ({})",
            found.display(),
            running.display()
        )),
        PathStatus::Missing { .. } => lines.push("  PATH: nh not found".to_owned()),
        PathStatus::Unknown(reason) => lines.push(format!("  PATH: unknown ({reason})")),
    }
    match &facts.catalog {
        CatalogStatus::Ready {
            location,
            route_count,
            provider_count,
            keys,
        } => {
            lines.push(format!("  catalog: {}", location.display()));
            lines.push(format!(
                "  routes: {route_count} across {provider_count} providers"
            ));
            match keys {
                KeyStatus::Entries(entries) if entries.is_empty() => {
                    lines.push("  keys: no API vault entries in catalog".to_owned());
                }
                KeyStatus::Entries(entries) => {
                    for key in entries {
                        let presence = if key.stored { "stored" } else { "absent" };
                        lines.push(format!(
                            "  key {}: {presence} (providers: {})",
                            key.entry,
                            key.providers.join(", ")
                        ));
                    }
                }
                KeyStatus::StoreUnavailable(reason) => {
                    lines.push(format!("  keys: OS key store unavailable ({reason})"));
                }
            }
        }
        CatalogStatus::Unreadable { location, reason } => {
            if let Some(location) = location {
                lines.push(format!(
                    "  catalog: could not be trusted at {} ({reason})",
                    location.display()
                ));
            } else {
                lines.push(format!("  catalog: could not be trusted ({reason})"));
            }
            lines.push("  routes: not checked".to_owned());
            lines.push("  keys: not checked".to_owned());
        }
    }
    lines.push(format!(
        "  stdout terminal: {}",
        yes_no(facts.stdout_terminal)
    ));
    lines.push(format!("  NO_COLOR: {}", set_state(facts.no_color)));
    if let Some(code_page) = facts.code_page {
        match code_page {
            CodePageStatus::Utf8 => lines.push("  Windows code page: 65001 (UTF-8)".to_owned()),
            CodePageStatus::Other(value) => {
                lines.push(format!("  Windows code page: {value} (not UTF-8)"));
            }
            CodePageStatus::Unknown => {
                lines.push("  Windows code page: unknown".to_owned());
            }
        }
    }
    for variable in &facts.network {
        lines.push(format!("  {}: {}", variable.name, set_state(variable.set)));
    }
    push_location(&mut lines, "user .nosis", &facts.user_config);
    push_location(&mut lines, "repository .nosis", &facts.repository_config);

    let mut next = Vec::new();
    match &facts.path {
        PathStatus::Running(_) => {}
        PathStatus::Missing { directory } => next.push(format!(
            "  PATH: nh is not on PATH - add {} to PATH, then restart your terminal",
            directory.display()
        )),
        PathStatus::Different { found, running } => next.push(format!(
            "  PATH: typing `nh` runs {}; this doctor is {} - put the current binary first on PATH, then restart your terminal",
            found.display(),
            running.display()
        )),
        PathStatus::Unknown(reason) => next.push(format!(
            "  PATH: {reason} - inspect PATH, then run `nh doctor` again"
        )),
    }
    match &facts.catalog {
        CatalogStatus::Ready { keys, .. } => match keys {
            KeyStatus::Entries(entries)
                if !entries.is_empty() && entries.iter().all(|entry| !entry.stored) =>
            {
                next.push(
                    "  keys: no API keys are stored - run `nh key add <entry>`; `nh why` needs no key at all"
                        .to_owned(),
                );
            }
            KeyStatus::StoreUnavailable(reason) => next.push(format!(
                "  key store: {reason} - unlock or enable the OS key store, then run `nh doctor` again"
            )),
            KeyStatus::Entries(_) => {}
        },
        CatalogStatus::Unreadable { reason, .. } => next.push(format!(
            "  catalog: {reason}; correct catalog.toml, then run `nh doctor` again; `nh init` still works"
        )),
    }
    if let Some(code_page) = facts.code_page {
        match code_page {
            CodePageStatus::Utf8 => {}
            CodePageStatus::Other(value) => next.push(format!(
                "  code page: {value} is not UTF-8 - box-drawing and non-ASCII text may render as mojibake; run `chcp 65001` before `nh`"
            )),
            CodePageStatus::Unknown => next.push(
                "  code page: unknown - run `chcp` to check it; use `chcp 65001` if it is not UTF-8"
                    .to_owned(),
            ),
        }
    }
    if next.is_empty() {
        next.push("  no problems found".to_owned());
    }
    lines.push(String::new());
    lines.push("what to do next:".to_owned());
    lines.extend(next);
    lines
}

fn push_location(lines: &mut Vec<String>, label: &str, location: &LocationStatus) {
    match location {
        LocationStatus::Exists(path) => {
            lines.push(format!("  {label}: {} (exists)", path.display()));
        }
        LocationStatus::Missing(path) => {
            lines.push(format!("  {label}: {} (not found)", path.display()));
        }
        LocationStatus::Unknown {
            path: Some(path),
            reason,
        } => lines.push(format!("  {label}: {} (unknown: {reason})", path.display())),
        LocationStatus::Unknown { path: None, reason } => {
            lines.push(format!("  {label}: unknown ({reason})"));
        }
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn set_state(value: bool) -> &'static str {
    if value {
        "set"
    } else {
        "not set"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy_facts() -> DoctorFacts {
        let binary = PathBuf::from("C:/tools/nh.exe");
        DoctorFacts {
            version: "0.2.0".to_owned(),
            binary: ProbeValue::Value(binary.clone()),
            path: PathStatus::Running(binary),
            catalog: CatalogStatus::Ready {
                location: PathBuf::from("C:/repo/catalog.toml"),
                route_count: 4,
                provider_count: 3,
                keys: KeyStatus::Entries(vec![KeyFact {
                    entry: "provider-one".to_owned(),
                    providers: vec!["provider-one".to_owned()],
                    stored: true,
                }]),
            },
            stdout_terminal: true,
            no_color: false,
            code_page: Some(CodePageStatus::Utf8),
            network: NETWORK_VARS
                .into_iter()
                .map(|name| NetworkFact { name, set: false })
                .collect(),
            user_config: LocationStatus::Exists(PathBuf::from("C:/user/.nosis")),
            repository_config: LocationStatus::Missing(PathBuf::from("C:/repo/.nosis")),
        }
    }

    fn next_lines(lines: &[String]) -> &[String] {
        let heading = lines
            .iter()
            .position(|line| line == "what to do next:")
            .unwrap();
        &lines[heading + 1..]
    }

    #[test]
    fn render_reports_all_facts_and_no_problem_line() {
        let lines = render(&healthy_facts());

        for expected in [
            "  version: 0.2.0",
            "  PATH: found running binary (C:/tools/nh.exe)",
            "  catalog: C:/repo/catalog.toml",
            "  routes: 4 across 3 providers",
            "  key provider-one: stored (providers: provider-one)",
            "  stdout terminal: yes",
            "  NO_COLOR: not set",
            "  Windows code page: 65001 (UTF-8)",
            "  HTTP_PROXY: not set",
            "  HTTPS_PROXY: not set",
            "  NO_PROXY: not set",
            "  SSL_CERT_FILE: not set",
            "  SSL_CERT_DIR: not set",
            "  user .nosis: C:/user/.nosis (exists)",
            "  repository .nosis: C:/repo/.nosis (not found)",
        ] {
            assert!(
                lines.iter().any(|line| line == expected),
                "missing {expected}"
            );
        }
        assert_eq!(next_lines(&lines), ["  no problems found"]);
    }

    #[test]
    fn render_gives_distinct_path_fixes() {
        let mut missing = healthy_facts();
        missing.path = PathStatus::Missing {
            directory: PathBuf::from("C:/current"),
        };
        let lines = render(&missing);
        assert_eq!(
            next_lines(&lines),
            ["  PATH: nh is not on PATH - add C:/current to PATH, then restart your terminal"]
        );

        let mut different = healthy_facts();
        different.path = PathStatus::Different {
            found: PathBuf::from("C:/stale/nh.exe"),
            running: PathBuf::from("C:/current/nh.exe"),
        };
        let lines = render(&different);
        let advice = next_lines(&lines).join("\n");
        assert!(advice.contains("typing `nh` runs C:/stale/nh.exe"));
        assert!(advice.contains("this doctor is C:/current/nh.exe"));
    }

    #[test]
    fn render_explains_missing_keys_and_non_utf8_code_page() {
        let mut facts = healthy_facts();
        let CatalogStatus::Ready { keys, .. } = &mut facts.catalog else {
            unreachable!();
        };
        *keys = KeyStatus::Entries(vec![KeyFact {
            entry: "provider-one".to_owned(),
            providers: vec!["provider-one".to_owned()],
            stored: false,
        }]);
        facts.code_page = Some(CodePageStatus::Other(437));

        let advice = next_lines(&render(&facts)).join("\n");
        assert!(advice.contains("run `nh key add <entry>`"));
        assert!(advice.contains("`nh why` needs no key at all"));
        assert!(advice.contains("code page: 437 is not UTF-8"));
        assert!(advice.contains("may render as mojibake"));
    }

    #[test]
    fn render_stays_useful_without_catalog_or_config() {
        let mut facts = healthy_facts();
        facts.catalog = CatalogStatus::Unreadable {
            location: None,
            reason: "no catalog.toml found - run `nh init` to create one".to_owned(),
        };
        facts.user_config = LocationStatus::Missing(PathBuf::from("C:/user/.nosis"));
        facts.repository_config = LocationStatus::Missing(PathBuf::from("C:/empty/.nosis"));

        let lines = render(&facts);
        let report = lines.join("\n");
        assert!(report.contains("version: 0.2.0"));
        assert!(report.contains("catalog: could not be trusted"));
        assert!(report.contains("routes: not checked"));
        assert!(report.contains("repository .nosis: C:/empty/.nosis (not found)"));
        assert!(report.contains("`nh init` still works"));
        assert!(!lines.is_empty());
    }

    #[test]
    fn render_reports_key_store_failure_without_calling_keys_absent() {
        let mut facts = healthy_facts();
        let CatalogStatus::Ready { keys, .. } = &mut facts.catalog else {
            unreachable!();
        };
        *keys = KeyStatus::StoreUnavailable("credential service is locked".to_owned());

        let lines = render(&facts);
        let report = lines.join("\n");
        assert!(report.contains("OS key store unavailable"));
        assert!(report.contains("unlock or enable the OS key store"));
        assert!(!report.contains("no API keys are stored"));
    }

    #[test]
    fn render_reports_unknown_code_page_without_guessing() {
        let mut facts = healthy_facts();
        facts.code_page = Some(CodePageStatus::Unknown);

        let lines = render(&facts);
        let report = lines.join("\n");
        assert!(report.contains("Windows code page: unknown"));
        assert!(!report.contains("Windows code page: 65001"));
        assert!(report.contains("run `chcp` to check it"));
    }

    #[cfg(windows)]
    #[test]
    fn code_page_parser_uses_the_trailing_integer() {
        assert_eq!(
            trailing_ascii_integer(b"Active code page: 437\r\n"),
            Some(437)
        );
        assert_eq!(
            trailing_ascii_integer(b"Active code page: 65001\r\n"),
            Some(65001)
        );
        assert_eq!(trailing_ascii_integer(b"code page unknown\r\n"), None);
    }
}
