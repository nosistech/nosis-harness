//! Crash-safe, append-only ledgers for interactive chat and TUI sessions.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Context as _;

use crate::wire::{ChatMessage, Usage};

const MAX_SESSION_ID_BYTES: usize = 128;
static NEXT_SESSION: AtomicU64 = AtomicU64::new(0);

/// Interactive surface that owns a session ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    Chat,
    Tui,
}

/// One append-only session record. Each value occupies one JSONL line.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SessionEvent {
    Started {
        session_id: String,
        surface: Surface,
        route_id: String,
        model_id: String,
        profile: String,
        created_utc: String,
    },
    Resumed {
        ts_utc: String,
    },
    RouteSwitched {
        ts_utc: String,
        route_id: String,
        model_id: String,
        profile: String,
    },
    Turn {
        ts_utc: String,
        route_id: String,
        messages: Vec<ChatMessage>,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
    },
    Ended {
        ts_utc: String,
    },
}

/// Meter input retained for replaying one completed or failed task.
#[derive(Debug, Clone)]
pub struct RestoredTurn {
    pub ts_utc: String,
    pub route_id: String,
    pub usage: Option<Usage>,
}

/// Folded state of one session ledger.
#[derive(Debug, Clone)]
pub struct RestoredSession {
    pub session_id: String,
    pub surface: Surface,
    pub route_id: String,
    pub model_id: String,
    pub profile: String,
    pub created_utc: String,
    pub history: Vec<ChatMessage>,
    pub turns: Vec<RestoredTurn>,
    pub ended: bool,
    pub dropped_torn_tail: bool,
}

/// One compact row for `nh resume` listing.
#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session_id: String,
    pub surface: Surface,
    pub route_id: String,
    pub model_id: String,
    pub profile: String,
    pub created_utc: String,
    pub last_ts_utc: String,
    pub turns: usize,
    pub ended: bool,
}

/// Readable sessions plus filenames that could not be folded.
#[derive(Debug, Default)]
pub struct SessionIndex {
    pub sessions: Vec<SessionSummary>,
    pub unreadable: Vec<String>,
}

/// Produce a filename-safe process-unique session id.
pub fn new_session_id() -> String {
    let created = chrono::Utc::now().format("%Y%m%dT%H%M%S%3fZ");
    let counter = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    format!("{created}-{}-{counter}", std::process::id())
}

/// Reject traversal and filesystem separators before an id reaches a path.
pub fn validate_session_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty() {
        anyhow::bail!("session id is empty - run `nh resume` to list sessions");
    }
    if id.len() > MAX_SESSION_ID_BYTES {
        anyhow::bail!("session id is too long - run `nh resume` to list sessions");
    }
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        anyhow::bail!("session id has invalid characters - run `nh resume` to list sessions");
    }
    Ok(())
}

/// Scrubbed append handle for `.nosis/sessions/<id>.jsonl`.
pub struct SessionLedger {
    root: PathBuf,
    path: PathBuf,
    scrubber: nh_vault::Scrubber,
    session_id: String,
}

impl SessionLedger {
    /// Select a session path. The directory is created only by the first append.
    pub fn create(
        root: impl Into<PathBuf>,
        session_id: impl Into<String>,
        scrubber: nh_vault::Scrubber,
    ) -> Self {
        let root = root.into();
        let session_id = session_id.into();
        let path = root
            .join(".nosis")
            .join("sessions")
            .join(format!("{session_id}.jsonl"));
        Self {
            root,
            path,
            scrubber,
            session_id,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn replace_scrubber(&mut self, scrubber: nh_vault::Scrubber) {
        self.scrubber = scrubber;
    }

    /// Append, flush, and fsync one scrubbed JSON object under an exclusive lock.
    pub fn append(&self, event: &SessionEvent) -> anyhow::Result<()> {
        validate_session_id(&self.session_id)?;
        let path = crate::runtime_path::ensure_contained_file(&self.root, &self.path, "session")?;
        let line = serde_json::to_string(event).context("could not serialize session record")?;
        let line = self.scrubber.scrub(&line);
        crate::jsonl::append_locked_line(&path, &line)
    }
}

/// Fold already-parsed records without touching the filesystem.
pub fn fold_session(events: &[SessionEvent]) -> anyhow::Result<RestoredSession> {
    let mut identity: Option<(String, Surface, String)> = None;
    let mut route_id = None;
    let mut model_id = None;
    let mut profile = None;
    let mut history = Vec::new();
    let mut turns = Vec::new();
    let mut ended = false;

    for event in events {
        match event {
            SessionEvent::Started {
                session_id,
                surface,
                route_id: next_route,
                model_id: next_model,
                profile: next_profile,
                created_utc,
            } => {
                identity.get_or_insert_with(|| (session_id.clone(), *surface, created_utc.clone()));
                route_id = Some(next_route.clone());
                model_id = Some(next_model.clone());
                profile = Some(next_profile.clone());
                ended = false;
            }
            SessionEvent::Resumed { .. } => ended = false,
            SessionEvent::RouteSwitched {
                route_id: next_route,
                model_id: next_model,
                profile: next_profile,
                ..
            } => {
                route_id = Some(next_route.clone());
                model_id = Some(next_model.clone());
                profile = Some(next_profile.clone());
            }
            SessionEvent::Turn {
                ts_utc,
                route_id,
                messages,
                usage,
            } => {
                history.extend(messages.iter().cloned());
                turns.push(RestoredTurn {
                    ts_utc: ts_utc.clone(),
                    route_id: route_id.clone(),
                    usage: usage.clone(),
                });
            }
            SessionEvent::Ended { .. } => ended = true,
        }
    }

    let Some((session_id, surface, created_utc)) = identity else {
        anyhow::bail!("session ledger has no start event");
    };
    Ok(RestoredSession {
        session_id,
        surface,
        route_id: route_id.context("session ledger has no route")?,
        model_id: model_id.context("session ledger has no model")?,
        profile: profile.context("session ledger has no profile")?,
        created_utc,
        history,
        turns,
        ended,
        dropped_torn_tail: false,
    })
}

/// Read and fold one ledger without repairing or rewriting it.
pub fn read_session(root: &Path, session_id: &str) -> anyhow::Result<RestoredSession> {
    validate_session_id(session_id)?;
    let Some(directory) =
        crate::runtime_path::resolve_contained_dir(root, Path::new(".nosis/sessions"))?
    else {
        anyhow::bail!("session {session_id} was not found - run `nh resume` to list sessions");
    };
    let path = directory.join(format!("{session_id}.jsonl"));
    crate::runtime_path::reject_symlink_or_special_file(&path, "session")?;
    let bytes =
        std::fs::read(&path).with_context(|| format!("could not read session {session_id}"))?;
    let (events, dropped_torn_tail) = parse_jsonl(&bytes)?;
    let mut restored = fold_session(&events)?;
    if restored.session_id != session_id {
        anyhow::bail!("session ledger id does not match its filename");
    }
    restored.dropped_torn_tail = dropped_torn_tail;
    Ok(restored)
}

/// List sessions newest-first. Missing storage is an empty, read-only result.
pub fn list_sessions(root: &Path) -> anyhow::Result<SessionIndex> {
    let Some(directory) =
        crate::runtime_path::resolve_contained_dir(root, Path::new(".nosis/sessions"))?
    else {
        return Ok(SessionIndex::default());
    };
    let mut index = SessionIndex::default();
    let entries = std::fs::read_dir(&directory)
        .with_context(|| format!("could not list {}", directory.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| format!("could not list {}", directory.display()))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        let filename = entry.file_name().to_string_lossy().into_owned();
        let Some(session_id) = path.file_stem().and_then(|value| value.to_str()) else {
            index.unreadable.push(filename);
            continue;
        };
        let restored = match read_session(root, session_id) {
            Ok(restored) => restored,
            Err(_) => {
                index.unreadable.push(filename);
                continue;
            }
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                index.unreadable.push(filename);
                continue;
            }
        };
        let (events, _) = match parse_jsonl(&bytes) {
            Ok(parsed) => parsed,
            Err(_) => {
                index.unreadable.push(filename);
                continue;
            }
        };
        let last_ts_utc = events
            .last()
            .map(event_timestamp)
            .unwrap_or(&restored.created_utc)
            .to_owned();
        index.sessions.push(SessionSummary {
            session_id: restored.session_id,
            surface: restored.surface,
            route_id: restored.route_id,
            model_id: restored.model_id,
            profile: restored.profile,
            created_utc: restored.created_utc,
            last_ts_utc,
            turns: restored.turns.len(),
            ended: restored.ended,
        });
    }
    index.sessions.sort_by(|left, right| {
        right
            .created_utc
            .cmp(&left.created_utc)
            .then_with(|| right.session_id.cmp(&left.session_id))
    });
    index.unreadable.sort();
    Ok(index)
}

fn event_timestamp(event: &SessionEvent) -> &str {
    match event {
        SessionEvent::Started { created_utc, .. } => created_utc,
        SessionEvent::Resumed { ts_utc }
        | SessionEvent::RouteSwitched { ts_utc, .. }
        | SessionEvent::Turn { ts_utc, .. }
        | SessionEvent::Ended { ts_utc } => ts_utc,
    }
}

fn parse_jsonl(bytes: &[u8]) -> anyhow::Result<(Vec<SessionEvent>, bool)> {
    crate::jsonl::parse_jsonl_records(bytes, |line, error| {
        anyhow::anyhow!("session ledger line {line} is invalid: {error}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started(id: &str) -> SessionEvent {
        SessionEvent::Started {
            session_id: id.to_owned(),
            surface: Surface::Chat,
            route_id: "test-route".to_owned(),
            model_id: "test-model".to_owned(),
            profile: "balanced".to_owned(),
            created_utc: "2026-07-31T14:05:02Z".to_owned(),
        }
    }

    fn message(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.to_owned(),
            content: Some(content.to_owned()),
            parts: None,
            tool_calls: None,
            tool_call_id: None,
            reasoning_content: (role == "assistant").then(|| "kept reasoning".to_owned()),
        }
    }

    fn ledger(root: &Path, id: &str) -> SessionLedger {
        SessionLedger::create(root, id, nh_vault::Scrubber::new(Vec::new()))
    }

    fn ledger_path(root: &Path, id: &str) -> PathBuf {
        root.join(".nosis")
            .join("sessions")
            .join(format!("{id}.jsonl"))
    }

    #[test]
    fn session_round_trip_preserves_history_bytes_exactly() {
        let root = tempfile::tempdir().unwrap();
        let id = "session-1";
        let original = vec![
            message("system", "original constitution"),
            message("user", "hello"),
            message("assistant", "answer"),
        ];
        let original_bytes = serde_json::to_vec(&original).unwrap();
        let writer = ledger(root.path(), id);
        writer.append(&started(id)).unwrap();
        writer
            .append(&SessionEvent::Turn {
                ts_utc: "2026-07-31T14:06:00Z".to_owned(),
                route_id: "test-route".to_owned(),
                messages: original,
                usage: Some(Usage {
                    prompt_tokens: 12,
                    completion_tokens: 4,
                    cached_tokens: Some(8),
                    evidence: crate::wire::UsageEvidence::Measured,
                }),
            })
            .unwrap();

        let restored = read_session(root.path(), id).unwrap();

        assert_eq!(
            serde_json::to_vec(&restored.history).unwrap(),
            original_bytes
        );
        assert!(!restored.dropped_torn_tail);
    }

    #[test]
    fn legacy_turn_usage_bytes_upgrade_to_unknown_evidence() {
        let old = br#"{"event":"turn","ts_utc":"2026-07-31T14:06:00Z","route_id":"test-route","messages":[],"usage":{"prompt_tokens":12,"completion_tokens":4,"cached_tokens":3}}"#;

        let parsed: SessionEvent = serde_json::from_slice(old).unwrap();
        let SessionEvent::Turn {
            usage: Some(usage), ..
        } = &parsed
        else {
            panic!("legacy turn must retain usage");
        };
        assert_eq!(usage.evidence, crate::wire::UsageEvidence::Unknown);

        let upgraded = serde_json::to_vec(&parsed).unwrap();
        assert_eq!(
            upgraded,
            br#"{"event":"turn","ts_utc":"2026-07-31T14:06:00Z","route_id":"test-route","messages":[],"usage":{"prompt_tokens":12,"completion_tokens":4,"cached_tokens":3,"evidence":"unknown"}}"#
        );
    }

    #[test]
    fn truncated_last_record_is_dropped_without_repairing_the_file() {
        let root = tempfile::tempdir().unwrap();
        let id = "torn-tail";
        let writer = ledger(root.path(), id);
        writer.append(&started(id)).unwrap();
        let path = ledger_path(root.path(), id);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(br#"{"event":"turn","ts_utc":"2026"#);
        std::fs::write(&path, &bytes).unwrap();

        let restored = read_session(root.path(), id).unwrap();

        assert!(restored.dropped_torn_tail);
        assert!(restored.turns.is_empty());
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn malformed_middle_record_names_line_and_reader_does_not_mutate() {
        let root = tempfile::tempdir().unwrap();
        let id = "bad-middle";
        let first = serde_json::to_string(&started(id)).unwrap();
        let last = serde_json::to_string(&SessionEvent::Ended {
            ts_utc: "2026-07-31T14:07:00Z".to_owned(),
        })
        .unwrap();
        let bytes = format!("{first}\n{{bad json}}\n{last}\n").into_bytes();
        let path = ledger_path(root.path(), id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &bytes).unwrap();

        let error = read_session(root.path(), id).unwrap_err();

        assert!(error.to_string().contains("line 2"), "got: {error}");
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn session_id_validation_blocks_traversal_before_filesystem_access() {
        for invalid in ["", "..", "../escape", r"..\escape", "with/slash", "a:b"] {
            assert!(
                validate_session_id(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
        assert!(validate_session_id("20260731T140502123Z-18244-0").is_ok());

        let root = tempfile::tempdir().unwrap().path().join("missing-root");
        let error = read_session(&root, "../escape").unwrap_err();
        assert!(error.to_string().contains("invalid characters"));
        assert!(!root.exists());
    }

    #[test]
    fn listing_missing_session_directory_is_empty_and_read_only() {
        let root = tempfile::tempdir().unwrap();

        let index = list_sessions(root.path()).unwrap();

        assert!(index.sessions.is_empty());
        assert!(index.unreadable.is_empty());
        assert!(!root.path().join(".nosis").exists());
    }

    #[test]
    fn listing_keeps_readable_sessions_when_one_file_is_bad() {
        let root = tempfile::tempdir().unwrap();
        let writer = ledger(root.path(), "good");
        writer.append(&started("good")).unwrap();
        let bad = ledger_path(root.path(), "broken");
        std::fs::write(&bad, b"{bad}\n").unwrap();

        let index = list_sessions(root.path()).unwrap();

        assert_eq!(index.sessions.len(), 1);
        assert_eq!(index.sessions[0].session_id, "good");
        assert_eq!(index.unreadable, ["broken.jsonl"]);
    }
}
