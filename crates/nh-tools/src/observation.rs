//! Session-scoped storage for explicitly retained, scrubbed tool observations.

use crate::{render_tool_result_without_observation, str_arg, Tool, ToolCtx, ToolSpec};
use anyhow::{bail, Context as _};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

const MAX_OBSERVATION_ITEMS: usize = 32;
const MAX_OBSERVATION_ITEM_BYTES: u64 = 4 * 1024 * 1024;
const MAX_OBSERVATION_SESSION_BYTES: u64 = 16 * 1024 * 1024;
const MAX_OBSERVATION_READ_CHARS: u64 = 6_000;
const MAX_OBSERVATION_READ_BYTES: usize = 24 * 1024;
const HANDLE_RANDOM_BYTES: usize = 16;

#[derive(Clone, Copy)]
struct ObservationLimits {
    items: usize,
    item_bytes: u64,
    session_bytes: u64,
}

impl Default for ObservationLimits {
    fn default() -> Self {
        Self {
            items: MAX_OBSERVATION_ITEMS,
            item_bytes: MAX_OBSERVATION_ITEM_BYTES,
            session_bytes: MAX_OBSERVATION_SESSION_BYTES,
        }
    }
}

#[derive(Clone)]
struct ObservationEntry {
    path: PathBuf,
    bytes: u64,
    chars: u64,
    digest: [u8; 32],
}

#[derive(Default)]
struct ObservationState {
    entries: HashMap<String, ObservationEntry>,
    owned_files: Vec<PathBuf>,
    total_bytes: u64,
    capture_disabled: bool,
    closed: bool,
}

struct ObservationInner {
    directory: PathBuf,
    boundary: Arc<()>,
    limits: ObservationLimits,
    state: Mutex<ObservationState>,
}

impl Drop for ObservationInner {
    fn drop(&mut self) {
        let _ = cleanup_inner(&self.directory, &self.state);
    }
}

/// One run's in-memory capability registry and owned scrubbed artifacts.
#[derive(Clone)]
pub struct ObservationSession {
    inner: Arc<ObservationInner>,
}

/// Scrubber-bound capability for retaining non-tool context in this session.
#[derive(Clone)]
pub struct ObservationRetainer {
    session: ObservationSession,
    scrubber: nh_vault::Scrubber,
}

impl ObservationRetainer {
    /// Scrub a field before a caller embeds it in an encoded archive format.
    /// The final serialized archive is scrubbed again by [`Self::retain`].
    pub fn scrub_field(&self, content: &str) -> String {
        self.scrubber.scrub(content)
    }

    /// Scrub and retain one bounded UTF-8 observation in the active session.
    pub fn retain(&self, content: &str) -> anyhow::Result<RetainedObservation> {
        let scrubbed = self.scrubber.scrub(content);
        self.session
            .capture(&scrubbed)
            .map_err(|failure| anyhow::anyhow!(failure.label()))
    }
}

impl ObservationSession {
    /// Create a private session directory below an already-contained runtime parent.
    pub fn create(runtime_parent: &Path, ctx: &ToolCtx) -> anyhow::Result<Self> {
        Self::create_with_limits(runtime_parent, ctx, ObservationLimits::default())
    }

    fn create_with_limits(
        runtime_parent: &Path,
        ctx: &ToolCtx,
        limits: ObservationLimits,
    ) -> anyhow::Result<Self> {
        let parent = std::fs::canonicalize(runtime_parent).with_context(|| {
            format!(
                "could not resolve observation runtime directory {}",
                runtime_parent.display()
            )
        })?;
        if !parent.is_dir() {
            bail!("observation runtime path is not a directory");
        }
        let directory = create_session_directory(&parent)?;
        let resolved = std::fs::canonicalize(&directory)
            .context("could not resolve new observation session directory")?;
        if !resolved.starts_with(&parent) {
            let _ = std::fs::remove_dir(&directory);
            bail!("refused observation session directory outside runtime parent");
        }
        Ok(Self {
            inner: Arc::new(ObservationInner {
                directory: resolved,
                boundary: Arc::clone(ctx.boundary()),
                limits,
                state: Mutex::new(ObservationState::default()),
            }),
        })
    }

    pub(crate) fn matches(&self, ctx: &ToolCtx) -> bool {
        Arc::ptr_eq(&self.inner.boundary, ctx.boundary())
    }

    pub(crate) fn same_session(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub(crate) fn retainer(&self, scrubber: nh_vault::Scrubber) -> ObservationRetainer {
        ObservationRetainer {
            session: self.clone(),
            scrubber,
        }
    }

    pub(crate) fn capture(&self, scrubbed: &str) -> Result<RetainedObservation, CaptureFailure> {
        verify_session_directory(&self.inner.directory).map_err(|_| CaptureFailure::Storage)?;
        let bytes = u64::try_from(scrubbed.len()).unwrap_or(u64::MAX);
        let chars = u64::try_from(scrubbed.chars().count()).unwrap_or(u64::MAX);
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| CaptureFailure::Unavailable)?;
        if state.closed || state.capture_disabled {
            return Err(CaptureFailure::Unavailable);
        }
        if bytes > self.inner.limits.item_bytes {
            return Err(CaptureFailure::ItemLimit);
        }
        if state.entries.len() >= self.inner.limits.items {
            return Err(CaptureFailure::ItemCount);
        }
        if state.total_bytes.saturating_add(bytes) > self.inner.limits.session_bytes {
            return Err(CaptureFailure::SessionLimit);
        }

        let digest: [u8; 32] = Sha256::digest(scrubbed.as_bytes()).into();
        let (handle, path, mut file) = create_observation_file(&self.inner.directory, &state)
            .map_err(|_| CaptureFailure::Storage)?;
        state.owned_files.push(path.clone());
        let write_result = (|| -> anyhow::Result<()> {
            file.write_all(scrubbed.as_bytes())
                .context("could not write retained observation")?;
            file.flush()
                .context("could not flush retained observation")?;
            file.sync_all()
                .context("could not sync retained observation")?;
            Ok(())
        })();
        drop(file);
        if write_result.is_err() {
            discard_failed_capture(&path, &mut state);
            return Err(CaptureFailure::Storage);
        }

        state.total_bytes = state.total_bytes.saturating_add(bytes);
        state.entries.insert(
            handle.clone(),
            ObservationEntry {
                path,
                bytes,
                chars,
                digest,
            },
        );
        Ok(RetainedObservation {
            handle,
            bytes,
            chars,
        })
    }

    fn retrieve(&self, handle: &str) -> anyhow::Result<(String, u64)> {
        verify_session_directory(&self.inner.directory)?;
        let entry = {
            let state = self
                .inner
                .state
                .lock()
                .map_err(|_| anyhow::anyhow!("observation session is unavailable"))?;
            if state.closed {
                bail!("observation session is closed");
            }
            state
                .entries
                .get(handle)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("unknown observation handle for this session"))?
        };
        reject_link_or_special(&entry.path)?;
        let file = std::fs::File::open(&entry.path)
            .context("could not open retained observation for verification")?;
        let opened = file
            .metadata()
            .context("could not inspect opened retained observation")?;
        if !opened.is_file() || opened.len() != entry.bytes {
            bail!("retained observation failed size verification");
        }
        let mut bytes = Vec::with_capacity(
            usize::try_from(entry.bytes.min(MAX_OBSERVATION_ITEM_BYTES)).unwrap_or(0),
        );
        file.take(entry.bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .context("could not read retained observation for verification")?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != entry.bytes {
            bail!("retained observation failed size verification");
        }
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        if digest != entry.digest {
            bail!("retained observation failed digest verification");
        }
        let content = String::from_utf8(bytes)
            .map_err(|_| anyhow::anyhow!("retained observation failed UTF-8 verification"))?;
        if u64::try_from(content.chars().count()).unwrap_or(u64::MAX) != entry.chars {
            bail!("retained observation failed character-count verification");
        }
        Ok((content, entry.chars))
    }

    /// Remove only files registered to this session, then its empty owned directory.
    pub fn cleanup(&self) -> anyhow::Result<()> {
        cleanup_inner(&self.inner.directory, &self.inner.state)
    }

    #[cfg(test)]
    fn directory(&self) -> &Path {
        &self.inner.directory
    }
}

fn discard_failed_capture(path: &Path, state: &mut ObservationState) {
    if std::fs::remove_file(path).is_ok() {
        state.owned_files.retain(|owned| owned != path);
    } else {
        // One bounded orphan remains owned for cleanup. Stop here so repeated
        // failures cannot accumulate beyond session quotas.
        state.capture_disabled = true;
    }
}

pub struct RetainedObservation {
    pub(crate) handle: String,
    pub(crate) bytes: u64,
    pub(crate) chars: u64,
}

impl RetainedObservation {
    pub fn handle(&self) -> &str {
        &self.handle
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    pub fn chars(&self) -> u64 {
        self.chars
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum CaptureFailure {
    ItemLimit,
    ItemCount,
    SessionLimit,
    Storage,
    Unavailable,
}

impl CaptureFailure {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::ItemLimit => "result exceeds the 4 MiB observation item limit",
            Self::ItemCount => "session reached the 32-observation item limit",
            Self::SessionLimit => "session reached the 16 MiB observation byte limit",
            Self::Storage => "local observation storage failed",
            Self::Unavailable => "observation session is unavailable",
        }
    }
}

pub(crate) struct ReadObservation {
    session: ObservationSession,
}

impl ReadObservation {
    pub(crate) fn new(session: ObservationSession) -> Self {
        Self { session }
    }
}

impl Tool for ReadObservation {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_observation".into(),
            description: "Read a bounded Unicode-character range from one scrubbed tool result retained during this run. Handles are session capabilities, not paths.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "handle": {
                        "type": "string",
                        "description": "Exact opaque handle emitted by a retained tool result."
                    },
                    "char_offset": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": MAX_OBSERVATION_ITEM_BYTES,
                        "description": "Zero-based Unicode-character offset."
                    },
                    "char_count": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_OBSERVATION_READ_CHARS,
                        "description": "Maximum Unicode characters to return."
                    }
                },
                "required": ["handle", "char_offset", "char_count"]
            }),
        }
    }

    fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> anyhow::Result<String> {
        if ctx.cancel.load(Ordering::Acquire) {
            return Ok(render_tool_result_without_observation(
                "turn cancelled before observation read".into(),
                ctx,
            ));
        }
        if !self.session.matches(ctx) || !ctx.has_observation_session(&self.session) {
            bail!("observation handle is not valid for this tool boundary");
        }
        let handle = str_arg(&args, "handle")?;
        let char_offset = whole_number_arg(&args, "char_offset", true)?;
        let char_count = whole_number_arg(&args, "char_count", false)?;
        if char_offset > MAX_OBSERVATION_ITEM_BYTES {
            bail!("char_offset exceeds the retained-observation item limit");
        }
        if char_count > MAX_OBSERVATION_READ_CHARS {
            bail!("char_count exceeds the {MAX_OBSERVATION_READ_CHARS}-character response limit");
        }
        let (content, total_chars) = self.session.retrieve(handle)?;
        if ctx.cancel.load(Ordering::Acquire) {
            return Ok(render_tool_result_without_observation(
                "turn cancelled before observation read".into(),
                ctx,
            ));
        }
        if char_offset >= total_chars {
            bail!(
                "char_offset {char_offset} is at or past end of observation ({total_chars} chars)"
            );
        }
        let end = char_offset.saturating_add(char_count).min(total_chars);
        let take = usize::try_from(end.saturating_sub(char_offset)).unwrap_or(usize::MAX);
        let skip = usize::try_from(char_offset).unwrap_or(usize::MAX);
        let selected = content.chars().skip(skip).take(take).collect::<String>();
        if selected.len() > MAX_OBSERVATION_READ_BYTES {
            bail!("selected observation range exceeds the 24 KiB response limit");
        }
        let end_marker = if end == total_chars {
            " (end reached)"
        } else {
            ""
        };
        Ok(render_tool_result_without_observation(
            format!(
                "chars {char_offset}-{end} of {total_chars} for {handle}{end_marker}:\n{selected}"
            ),
            ctx,
        ))
    }
}

fn whole_number_arg(
    args: &serde_json::Value,
    name: &str,
    zero_allowed: bool,
) -> anyhow::Result<u64> {
    let value = args
        .get(name)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("{name} must be a non-negative whole number"))?;
    if !zero_allowed && value == 0 {
        bail!("{name} must be a positive whole number");
    }
    Ok(value)
}

fn cleanup_inner(directory: &Path, state: &Mutex<ObservationState>) -> anyhow::Result<()> {
    {
        let state = state
            .lock()
            .map_err(|_| anyhow::anyhow!("observation session cleanup state is unavailable"))?;
        if state.closed {
            return Ok(());
        }
    }
    verify_session_directory(directory)?;
    let files = {
        let mut state = state
            .lock()
            .map_err(|_| anyhow::anyhow!("observation session cleanup state is unavailable"))?;
        if state.closed {
            return Ok(());
        }
        state.closed = true;
        state.entries.clear();
        state.total_bytes = 0;
        std::mem::take(&mut state.owned_files)
    };
    let mut first_error = None;
    for path in files {
        if let Err(error) = std::fs::remove_file(&path) {
            if error.kind() != std::io::ErrorKind::NotFound && first_error.is_none() {
                first_error = Some(anyhow::Error::new(error).context(format!(
                    "could not remove retained observation {}",
                    path.display()
                )));
            }
        }
    }
    if let Err(error) = std::fs::remove_dir(directory) {
        if error.kind() != std::io::ErrorKind::NotFound && first_error.is_none() {
            first_error = Some(anyhow::Error::new(error).context(format!(
                "could not remove observation session directory {}",
                directory.display()
            )));
        }
    }
    first_error.map_or(Ok(()), Err)
}

fn create_session_directory(parent: &Path) -> anyhow::Result<PathBuf> {
    for _ in 0..16 {
        let path = parent.join(format!("session-{}", random_hex()?));
        match create_private_directory(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).context("could not create private observation session directory")
            }
        }
    }
    bail!("could not allocate a unique observation session directory")
}

fn create_observation_file(
    directory: &Path,
    state: &ObservationState,
) -> anyhow::Result<(String, PathBuf, std::fs::File)> {
    for _ in 0..16 {
        let handle = format!("obs_{}", random_hex()?);
        if state.entries.contains_key(&handle) {
            continue;
        }
        let path = directory.join(format!("{handle}.txt"));
        match create_private_file(&path) {
            Ok(file) => return Ok((handle, path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).context("could not create private retained observation")
            }
        }
    }
    bail!("could not allocate a unique observation handle")
}

fn random_hex() -> anyhow::Result<String> {
    let mut bytes = [0_u8; HANDLE_RANDOM_BYTES];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| anyhow::anyhow!("could not obtain observation randomness: {error}"))?;
    let mut encoded = String::with_capacity(HANDLE_RANDOM_BYTES * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(encoded)
}

fn reject_link_or_special(path: &Path) -> anyhow::Result<()> {
    let metadata = std::fs::symlink_metadata(path)
        .context("could not inspect retained observation before reading")?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("retained observation path is not a regular file");
    }
    Ok(())
}

fn verify_session_directory(path: &Path) -> anyhow::Result<()> {
    let metadata = std::fs::symlink_metadata(path)
        .context("could not inspect observation session directory")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("observation session path is not a regular directory");
    }
    let resolved =
        std::fs::canonicalize(path).context("could not resolve observation session directory")?;
    if resolved != path {
        bail!("observation session directory identity changed");
    }
    Ok(())
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_directory(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir(path)
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true).mode(0o600).open(path)
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Access, Guard, GuardFn};

    fn guard() -> GuardFn {
        Box::new(|access| match access {
            Access::Read(_) | Access::Write(_) | Access::Exec(_) | Access::Send(_) => Guard::Allow,
        })
    }

    fn ctx(root: &Path) -> ToolCtx {
        ToolCtx::new(
            root.to_path_buf(),
            Box::new(|_| false),
            guard(),
            nh_vault::Scrubber::new(Vec::new()),
        )
    }

    #[test]
    fn quota_refusal_does_not_create_an_unregistered_file() {
        let root = tempfile::tempdir().unwrap();
        let ctx = ctx(root.path());
        let session = ObservationSession::create_with_limits(
            root.path(),
            &ctx,
            ObservationLimits {
                items: 1,
                item_bytes: 8,
                session_bytes: 8,
            },
        )
        .unwrap();

        assert!(matches!(
            session.capture("123456789"),
            Err(CaptureFailure::ItemLimit)
        ));
        assert_eq!(std::fs::read_dir(session.directory()).unwrap().count(), 0);
    }

    #[test]
    fn quota_failure_falls_back_to_the_existing_bounded_envelope() {
        let root = tempfile::tempdir().unwrap();
        let ctx = ctx(root.path());
        let session = ObservationSession::create_with_limits(
            root.path(),
            &ctx,
            ObservationLimits {
                items: 1,
                item_bytes: 8,
                session_bytes: 8,
            },
        )
        .unwrap();
        let directory = session.directory().to_path_buf();
        let ctx = ctx.with_observation_session(session).unwrap();

        let output = crate::render_tool_result("x".repeat(33_000), &ctx);

        assert!(output.starts_with(
            "[full observation unavailable: result exceeds the 4 MiB observation item limit]"
        ));
        assert!(output.contains("chars elided; digest "));
        assert_eq!(std::fs::read_dir(directory).unwrap().count(), 0);
    }

    #[test]
    fn item_count_and_session_byte_quotas_are_enforced_before_creation() {
        let root = tempfile::tempdir().unwrap();
        let ctx = ctx(root.path());
        let item_session = ObservationSession::create_with_limits(
            root.path(),
            &ctx,
            ObservationLimits {
                items: 1,
                item_bytes: 10,
                session_bytes: 10,
            },
        )
        .unwrap();
        item_session.capture("123").unwrap();
        assert!(matches!(
            item_session.capture("456"),
            Err(CaptureFailure::ItemCount)
        ));
        assert_eq!(
            std::fs::read_dir(item_session.directory()).unwrap().count(),
            1
        );

        let byte_session = ObservationSession::create_with_limits(
            root.path(),
            &ctx,
            ObservationLimits {
                items: 2,
                item_bytes: 10,
                session_bytes: 5,
            },
        )
        .unwrap();
        byte_session.capture("123").unwrap();
        assert!(matches!(
            byte_session.capture("456"),
            Err(CaptureFailure::SessionLimit)
        ));
        assert_eq!(
            std::fs::read_dir(byte_session.directory()).unwrap().count(),
            1
        );
    }

    #[test]
    fn failed_orphan_cleanup_latches_capture_disabled() {
        let root = tempfile::tempdir().unwrap();
        let orphan = root.path().join("orphan-directory");
        std::fs::create_dir(&orphan).unwrap();
        let mut state = ObservationState {
            owned_files: vec![orphan.clone()],
            ..ObservationState::default()
        };

        discard_failed_capture(&orphan, &mut state);

        assert!(state.capture_disabled);
        assert_eq!(state.owned_files, [orphan]);
    }
}
