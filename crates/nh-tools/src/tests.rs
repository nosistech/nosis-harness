use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Instant;

fn ctx_with(workdir: &Path, approve: bool) -> ToolCtx {
    ToolCtx::new(
        workdir.to_path_buf(),
        Box::new(move |_| approve),
        permissive_test_guard(),
        empty_test_scrubber(),
    )
}

fn permissive_test_guard() -> GuardFn {
    Box::new(|access| match access {
        Access::Read(_) | Access::Write(_) | Access::Send(_) => Guard::Allow,
        Access::Exec(_) => Guard::Ask,
    })
}

fn empty_test_scrubber() -> nh_vault::Scrubber {
    nh_vault::Scrubber::new(Vec::new())
}

fn observation_context(
    workdir: &Path,
    runtime_parent: &Path,
    approve: bool,
    scrubber: nh_vault::Scrubber,
) -> (ToolCtx, ObservationSession) {
    std::fs::create_dir_all(runtime_parent).unwrap();
    let ctx = ToolCtx::new(
        workdir.to_path_buf(),
        Box::new(move |_| approve),
        permissive_test_guard(),
        scrubber,
    );
    let session = ObservationSession::create(runtime_parent, &ctx).unwrap();
    let ctx = ctx.with_observation_session(session.clone()).unwrap();
    (ctx, session)
}

fn observation_handle(output: &str) -> String {
    output
        .split("handle=")
        .nth(1)
        .and_then(|suffix| suffix.split(';').next())
        .expect("retained output must expose one opaque handle")
        .to_owned()
}

fn observation_tool(session: &ObservationSession) -> Box<dyn Tool> {
    builtin_tools_with_features(ToolFeatures {
        ranged_reads: false,
        observation_session: Some(session.clone()),
    })
    .into_iter()
    .find(|tool| tool.spec().name == "read_observation")
    .expect("observation-enabled registry must include read_observation")
}

fn only_observation_file(runtime_parent: &Path) -> PathBuf {
    let session_dir = std::fs::read_dir(runtime_parent)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.is_dir())
        .expect("one observation session directory");
    let files = std::fs::read_dir(session_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 1, "expected exactly one retained observation");
    files.into_iter().next().unwrap()
}

#[test]
fn policy_guard_keeps_bundled_read_blocks() {
    let repo = tempfile::tempdir().unwrap();
    let law = nh_law::load(repo.path(), &nh_law::LoadOptions { cli_autonomy: None });
    let guard = policy_guard(law.policy);

    assert!(matches!(
        guard(&Access::Read(".git/config")),
        Guard::Block(_)
    ));
}

#[cfg(windows)]
#[test]
fn bundled_policy_blocks_existing_uppercase_secret_paths_on_windows() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".GIT")).unwrap();
    std::fs::create_dir(repo.path().join("nested")).unwrap();
    for path in [".GIT/HEAD", ".ENV", "nested/B.PEM"] {
        std::fs::write(repo.path().join(path), "private").unwrap();
    }
    let law = nh_law::load(repo.path(), &nh_law::LoadOptions { cli_autonomy: None });
    let ctx = ToolCtx::new(
        repo.path().to_path_buf(),
        Box::new(|_| panic!("protected paths must not reach approval")),
        policy_guard(law.policy),
        empty_test_scrubber(),
    );

    for path in [".GIT/HEAD", ".ENV", "nested/B.PEM"] {
        let read = ReadFile.execute(json!({"path": path}), &ctx).unwrap();
        assert!(read.starts_with("blocked by law:"), "{path}: {read}");

        let edit = EditFile
            .execute(
                json!({"path": path, "old_string": "private", "new_string": "changed"}),
                &ctx,
            )
            .unwrap();
        assert!(edit.starts_with("blocked by law:"), "{path}: {edit}");
        assert_eq!(
            std::fs::read_to_string(repo.path().join(path)).unwrap(),
            "private"
        );
    }
}

#[test]
fn read_only_guard_preserves_read_policy_and_blocks_other_access() {
    let guard = read_only_guard(Box::new(|access| match access {
        Access::Read("private.txt") => Guard::Block("operator read restriction".into()),
        _ => Guard::Allow,
    }));

    assert!(matches!(guard(&Access::Read("public.txt")), Guard::Allow));
    match guard(&Access::Read("private.txt")) {
        Guard::Block(reason) => assert_eq!(reason, "operator read restriction"),
        _ => panic!("the existing read restriction must remain effective"),
    }
    for access in [
        Access::Write("change.txt"),
        Access::Exec("echo change"),
        Access::Send("remote-tool"),
    ] {
        match guard(&access) {
            Guard::Block(reason) => assert_eq!(
                reason,
                "read-only run blocks writes, commands, and remote tools"
            ),
            _ => panic!("read-only access was not blocked"),
        }
    }
}

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

fn png_bytes() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(b"fixture");
    bytes
}

#[test]
fn base64_matches_rfc_4648_vectors() {
    for (plain, encoded) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(encode_base64(plain.as_bytes()), encoded, "{plain:?}");
    }
}

#[test]
fn base64_full_byte_range_has_standard_length_and_alphabet() {
    let bytes: Vec<u8> = (0..=u8::MAX).collect();
    let encoded = encode_base64(&bytes);

    assert_eq!(encoded.len(), bytes.len().div_ceil(3) * 4);
    assert!(
        encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')),
        "non-RFC-4648 byte in {encoded}"
    );
    let padding = encoded
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'=')
        .count();
    assert_eq!(padding, 2);
    assert!(!encoded[..encoded.len() - padding].contains('='));
}

#[test]
fn load_image_validates_and_encodes_png_and_jpeg() {
    let dir = tempfile::tempdir().unwrap();
    let png = png_bytes();
    let jpeg = b"\xff\xd8\xfffixture".to_vec();
    std::fs::write(dir.path().join("screen.png"), &png).unwrap();
    std::fs::write(dir.path().join("photo.jpeg"), &jpeg).unwrap();
    let ctx = ctx_with(dir.path(), true);

    assert_eq!(
        load_image("screen.png", &ctx).unwrap(),
        LoadedImage {
            media_type: "image/png".into(),
            data: encode_base64(&png),
        }
    );
    assert_eq!(
        load_image("photo.jpeg", &ctx).unwrap(),
        LoadedImage {
            media_type: "image/jpeg".into(),
            data: encode_base64(&jpeg),
        }
    );
}

#[test]
fn load_image_refuses_workdir_escape() {
    let dir = tempfile::tempdir().unwrap();
    let workdir = dir.path().join("inner");
    std::fs::create_dir(&workdir).unwrap();
    std::fs::write(dir.path().join("outside.png"), png_bytes()).unwrap();

    let error = load_image("../outside.png", &ctx_with(&workdir, true))
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("escapes the working directory"),
        "got: {error}"
    );
}

#[test]
fn load_image_refuses_law_block_before_reading() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("blocked.png"), png_bytes()).unwrap();
    let approvals = Arc::new(AtomicUsize::new(0));
    let approvals_seen = Arc::clone(&approvals);
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |_| {
            approvals_seen.fetch_add(1, Ordering::SeqCst);
            true
        }),
        Box::new(|access| match access {
            Access::Read("blocked.png") => Guard::Block("protected image".into()),
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );

    let error = load_image("blocked.png", &ctx).unwrap_err().to_string();

    assert_eq!(error, "blocked by law: protected image");
    assert_eq!(approvals.load(Ordering::SeqCst), 0);
}

#[test]
fn load_image_refuses_unsupported_extension() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("screen.gif"), png_bytes()).unwrap();

    let error = load_image("screen.gif", &ctx_with(dir.path(), true))
        .unwrap_err()
        .to_string();

    assert_eq!(
        error,
        "unsupported image format - use PNG or JPEG (.png, .jpg, .jpeg)"
    );
}

#[test]
fn load_image_refuses_magic_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("disguised.png"), b"\xff\xd8\xfffixture").unwrap();

    let error = load_image("disguised.png", &ctx_with(dir.path(), true))
        .unwrap_err()
        .to_string();

    assert_eq!(error, "image bytes do not match the .png extension");
}

#[test]
fn load_image_refuses_oversize_raw_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = png_bytes();
    bytes.resize(MAX_IMAGE_BYTES + 1, 0);
    std::fs::write(dir.path().join("large.png"), bytes).unwrap();

    let error = load_image("large.png", &ctx_with(dir.path(), true))
        .unwrap_err()
        .to_string();

    assert_eq!(
        error,
        format!("image is too large - maximum raw size is 3.5 MiB ({MAX_IMAGE_BYTES} bytes)")
    );
}

struct BlockingReader {
    first: Option<Vec<u8>>,
    blocked: Option<mpsc::Sender<()>>,
    release: mpsc::Receiver<()>,
}

impl Read for BlockingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if let Some(mut bytes) = self.first.take() {
            let read = bytes.len().min(buffer.len());
            buffer[..read].copy_from_slice(&bytes[..read]);
            if read < bytes.len() {
                bytes.drain(..read);
                self.first = Some(bytes);
            }
            return Ok(read);
        }
        if let Some(blocked) = self.blocked.take() {
            let _ = blocked.send(());
        }
        let _ = self.release.recv();
        Ok(0)
    }
}

#[test]
fn specs_have_expected_names_and_required_args() {
    let tools = builtin_tools();
    let specs: Vec<ToolSpec> = tools.iter().map(|t| t.spec()).collect();
    let names: Vec<&str> = specs.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "read_file",
            "write_file",
            "edit_file",
            "grep_files",
            "glob_files",
            "exec_shell"
        ]
    );
    assert_eq!(specs[0].parameters["required"], json!(["path"]));
    assert_eq!(specs[1].parameters["required"], json!(["path", "content"]));
    assert_eq!(
        specs[2].parameters["required"],
        json!(["path", "old_string", "new_string"])
    );
    assert_eq!(specs[3].parameters["required"], json!(["pattern"]));
    assert_eq!(specs[4].parameters["required"], json!(["pattern"]));
    assert_eq!(specs[5].parameters["required"], json!(["command"]));
    assert!(specs[3]
        .description
        .contains("literal substring, not a regular expression"));
    for spec in [&specs[3], &specs[4]] {
        for directory in ["target", "node_modules", ".venv", "dist", "build"] {
            assert!(
                spec.description.contains(directory),
                "{:?}",
                spec.description
            );
        }
    }
    for spec in &specs {
        assert_eq!(spec.parameters["type"], "object");
        assert!(!spec.description.is_empty());
    }
}

#[test]
fn policy_filtered_registry_omits_shell_only_for_proven_complete_denial() {
    fn loaded_policy(block: &str) -> nh_law::Policy {
        let repo = tempfile::tempdir().unwrap();
        std::fs::create_dir(repo.path().join(".nosis")).unwrap();
        std::fs::write(
            repo.path().join(".nosis/law.toml"),
            format!("[exec]\nblock = [{block:?}]\n"),
        )
        .unwrap();
        nh_law::load(repo.path(), &nh_law::LoadOptions { cli_autonomy: None }).policy
    }

    let denied = builtin_tools_for_policy(&loaded_policy("*"))
        .into_iter()
        .map(|tool| tool.spec().name)
        .collect::<Vec<_>>();
    assert_eq!(
        denied,
        [
            "read_file",
            "write_file",
            "edit_file",
            "grep_files",
            "glob_files"
        ]
    );

    let partial = builtin_tools_for_policy(&loaded_policy("cargo *"))
        .into_iter()
        .map(|tool| tool.spec().name)
        .collect::<Vec<_>>();
    assert_eq!(partial.last().map(String::as_str), Some("exec_shell"));
}

#[test]
fn read_only_registry_has_exactly_the_three_local_read_tools() {
    let names = read_only_tools()
        .iter()
        .map(|tool| tool.spec().name)
        .collect::<Vec<_>>();

    assert_eq!(names, ["read_file", "glob_files", "grep_files"]);
}

#[test]
fn observation_tool_is_opt_in_and_short_results_stay_identical() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("runtime");
    std::fs::write(
        dir.path().join("short.txt"),
        "short\rresult\twith caf\u{e9} \u{6f22}\u{5b57} \x1b[31mred\x1b[0m and bell\x07",
    )
    .unwrap();
    let default = ReadFile
        .execute(json!({"path": "short.txt"}), &ctx_with(dir.path(), true))
        .unwrap();
    let (ctx, session) = observation_context(
        dir.path(),
        &runtime,
        true,
        nh_vault::Scrubber::new(Vec::new()),
    );
    let observed = ReadFile
        .execute(json!({"path": "short.txt"}), &ctx)
        .unwrap();
    assert_eq!(observed, default);

    let tools = read_only_tools_with_features(ToolFeatures {
        ranged_reads: false,
        observation_session: Some(session),
    });
    let names = tools
        .iter()
        .map(|tool| tool.spec().name)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["read_file", "glob_files", "grep_files", "read_observation"]
    );
    let spec = tools.last().unwrap().spec();
    assert_eq!(
        spec.parameters["required"],
        json!(["handle", "char_offset", "char_count"])
    );
    assert_eq!(spec.parameters["properties"]["char_offset"]["minimum"], 0);
    assert_eq!(spec.parameters["properties"]["char_count"]["maximum"], 6000);
}

#[test]
fn read_only_guard_stops_accidentally_registered_mutators_before_approval() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "before").unwrap();
    let approvals = Arc::new(AtomicUsize::new(0));
    let approvals_seen = Arc::clone(&approvals);
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |_| {
            approvals_seen.fetch_add(1, Ordering::SeqCst);
            true
        }),
        read_only_guard(Box::new(|_| Guard::Allow)),
        empty_test_scrubber(),
    );
    let blocked = "blocked by law: read-only run blocks writes, commands, and remote tools";

    assert_eq!(
        WriteFile
            .execute(
                json!({"path": "created.txt", "content": "unexpected"}),
                &ctx,
            )
            .unwrap(),
        blocked
    );
    assert_eq!(
        EditFile
            .execute(
                json!({"path": "note.txt", "old_string": "before", "new_string": "after"}),
                &ctx,
            )
            .unwrap(),
        blocked
    );
    assert_eq!(
        ExecShell
            .execute(json!({"command": "echo unexpected > marker.txt"}), &ctx)
            .unwrap(),
        blocked
    );
    assert_eq!(approvals.load(Ordering::SeqCst), 0);
    assert!(!dir.path().join("created.txt").exists());
    assert!(!dir.path().join("marker.txt").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("note.txt")).unwrap(),
        "before"
    );
}

#[test]
fn write_file_creates_a_new_file_without_temp_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("src");
    std::fs::create_dir(&source).unwrap();

    let execution = WriteFile
        .execute_with_audit(
            json!({"path": "src/new_module.rs", "content": "pub fn new() {}\n"}),
            &ctx_with(dir.path(), true),
        )
        .unwrap();

    assert_eq!(execution.output, "created src/new_module.rs (16 bytes)");
    assert_eq!(
        execution.audit,
        vec![ToolAudit::FilePublished(FileChangeKind::Created)]
    );
    assert_eq!(
        std::fs::read_to_string(source.join("new_module.rs")).unwrap(),
        "pub fn new() {}\n"
    );
    assert!(std::fs::read_dir(source).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".nh-write-")
    }));
}

#[test]
fn temporary_file_paths_are_distinct_and_non_collision_errors_surface() {
    let dir = tempfile::tempdir().unwrap();
    let (first_path, first_file) =
        create_temp_file(dir.path(), ".nh-test-", 17, "note.txt").unwrap();
    let (second_path, second_file) =
        create_temp_file(dir.path(), ".nh-test-", 17, "note.txt").unwrap();
    assert_ne!(first_path, second_path);
    drop((first_file, second_file));

    let not_directory = dir.path().join("not-a-directory");
    std::fs::write(&not_directory, "unchanged").unwrap();
    let error = create_temp_file(&not_directory, ".nh-test-", 18, "blocked.txt").unwrap_err();
    assert_eq!(
        error.to_string(),
        "could not create temporary file for blocked.txt"
    );
    let io_error = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<std::io::Error>())
        .expect("temporary-file failure must retain its I/O source");
    assert_ne!(io_error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(not_directory).unwrap(), "unchanged");
}

#[test]
fn create_publication_never_replaces_a_competing_file() {
    use std::io::Write as _;

    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("note.txt");
    let (temp_path, mut temp_file) =
        create_temp_file(dir.path(), ".nh-write-", 19, "note.txt").unwrap();
    temp_file.write_all(b"staged bytes").unwrap();
    temp_file.flush().unwrap();
    temp_file.sync_all().unwrap();
    drop(temp_file);

    let publication = publish_create_only_with(&temp_path, &destination, "note.txt", || {
        std::fs::write(&destination, b"competitor bytes")?;
        Ok(())
    })
    .unwrap();

    assert_eq!(publication, CreatePublication::DestinationExists);
    assert_eq!(std::fs::read(&destination).unwrap(), b"competitor bytes");
    assert!(!temp_path.exists(), "staging file was not cleaned");
}

#[test]
fn edit_publication_refuses_a_concurrent_change_and_cleans_staging() {
    use std::io::Write as _;

    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("note.txt");
    std::fs::write(&destination, b"alpha payload").unwrap();
    let metadata = std::fs::File::open(&destination)
        .unwrap()
        .metadata()
        .unwrap();
    let original_modified = metadata.modified().unwrap();
    let (temp_path, mut temp_file) =
        create_edit_temp_file(dir.path(), ".nh-edit-", 20, "note.txt").unwrap();
    temp_file.write_all(b"edited bytes").unwrap();
    temp_file.flush().unwrap();
    temp_file.sync_all().unwrap();
    drop(temp_file);

    let publication = publish_edit_with(
        &temp_path,
        &destination,
        b"alpha payload",
        &metadata,
        "note.txt",
        || {
            std::fs::write(&destination, b"omega payload")?;
            std::fs::File::options()
                .write(true)
                .open(&destination)?
                .set_times(std::fs::FileTimes::new().set_modified(original_modified))?;
            Ok(())
        },
        || false,
    )
    .unwrap();

    assert_eq!(publication, EditPublication::Conflict);
    assert_eq!(std::fs::read(&destination).unwrap(), b"omega payload");
    assert!(!temp_path.exists(), "staging file was not cleaned");
}

#[test]
fn edit_publication_observes_cancellation_after_recheck() {
    use std::io::Write as _;

    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("note.txt");
    std::fs::write(&destination, b"original bytes").unwrap();
    let metadata = std::fs::File::open(&destination)
        .unwrap()
        .metadata()
        .unwrap();
    let (temp_path, mut temp_file) =
        create_edit_temp_file(dir.path(), ".nh-edit-", 21, "note.txt").unwrap();
    temp_file.write_all(b"edited bytes").unwrap();
    temp_file.flush().unwrap();
    temp_file.sync_all().unwrap();
    drop(temp_file);
    let cancelled = AtomicBool::new(false);

    let publication = publish_edit_with(
        &temp_path,
        &destination,
        b"original bytes",
        &metadata,
        "note.txt",
        || Ok(()),
        || {
            cancelled.store(true, Ordering::Release);
            cancelled.load(Ordering::Acquire)
        },
    )
    .unwrap();

    assert_eq!(publication, EditPublication::Cancelled);
    assert_eq!(std::fs::read(&destination).unwrap(), b"original bytes");
    assert!(!temp_path.exists(), "staging file was not cleaned");
}

#[test]
fn write_file_refuses_existing_file_and_names_edit_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note.txt");
    std::fs::write(&path, "before").unwrap();

    let result = WriteFile
        .execute(
            json!({"path": "note.txt", "content": "after"}),
            &ctx_with(dir.path(), true),
        )
        .unwrap();

    assert_eq!(
        result,
        "refused: note.txt already exists - use edit_file to change it"
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "before");
}

#[test]
fn write_file_refuses_missing_parent_without_creating_it() {
    let dir = tempfile::tempdir().unwrap();

    let result = WriteFile
        .execute(
            json!({"path": "missing/note.txt", "content": "hello"}),
            &ctx_with(dir.path(), true),
        )
        .unwrap();

    assert_eq!(
        result,
        "refused: parent directory does not exist: missing - create it first"
    );
    assert!(!dir.path().join("missing").exists());
}

#[test]
fn write_file_refuses_workdir_escape() {
    let dir = tempfile::tempdir().unwrap();
    let workdir = dir.path().join("inner");
    std::fs::create_dir(&workdir).unwrap();

    let error = WriteFile
        .execute(
            json!({"path": "../outside.txt", "content": "no"}),
            &ctx_with(&workdir, true),
        )
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("escapes the working directory"),
        "got: {error}"
    );
    assert!(!dir.path().join("outside.txt").exists());
}

#[test]
fn write_file_case_folds_guard_paths_and_still_allows_normal_creation() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".GIT")).unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    let approvals = Arc::new(AtomicUsize::new(0));
    let approvals_seen = Arc::clone(&approvals);
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |_| {
            approvals_seen.fetch_add(1, Ordering::SeqCst);
            true
        }),
        Box::new(|access| match access {
            Access::Write(path)
                if nh_law::glob_matches(".git/**", path)
                    || nh_law::glob_matches("**/.env*", path) =>
            {
                Guard::Block("protected creation path".into())
            }
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );

    for path in [".GIT/x", ".ENV", ".Env.local"] {
        let result = WriteFile
            .execute(json!({"path": path, "content": "blocked"}), &ctx)
            .unwrap();
        assert_eq!(result, "blocked by law: protected creation path");
        assert!(!dir.path().join(path).exists(), "{path} was created");
    }

    let allowed = WriteFile
        .execute(
            json!({"path": "src/new_module.rs", "content": "safe"}),
            &ctx,
        )
        .unwrap();
    assert_eq!(allowed, "created src/new_module.rs (4 bytes)");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/new_module.rs")).unwrap(),
        "safe"
    );
    assert_eq!(approvals.load(Ordering::SeqCst), 0);
}

#[test]
fn write_file_lowercase_ask_beats_typed_allow_and_denial_is_ok_shaped() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("Notes")).unwrap();
    let actions = Arc::new(std::sync::Mutex::new(Vec::new()));
    let actions_seen = Arc::clone(&actions);
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |action| {
            actions_seen.lock().unwrap().push(action.to_owned());
            false
        }),
        Box::new(|access| match access {
            Access::Write("notes/new.txt") => Guard::Ask,
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );

    let result = WriteFile
        .execute(json!({"path": "Notes/New.txt", "content": "no"}), &ctx)
        .unwrap();

    assert_eq!(result, "user denied: create Notes/New.txt");
    assert_eq!(*actions.lock().unwrap(), ["create Notes/New.txt"]);
    assert!(!dir.path().join("Notes/New.txt").exists());
}

#[test]
fn exec_cancellation_before_approval_is_typed_as_not_started() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = Arc::new(AtomicBool::new(true));
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(|_| panic!("pre-cancelled command must not ask for approval")),
        Box::new(|_| panic!("pre-cancelled command must not consult the guard")),
        empty_test_scrubber(),
    )
    .with_cancel(cancel);

    let execution = ExecShell
        .execute_with_audit(json!({"command": "echo should-not-run > marker.txt"}), &ctx)
        .unwrap();

    assert_eq!(execution.output, "turn cancelled before command execution");
    assert_eq!(
        execution.audit,
        vec![ToolAudit::Command(CommandOutcome::CancelledBeforeStart)]
    );
    assert!(!dir.path().join("marker.txt").exists());
}

#[test]
fn cancellation_after_approval_stops_file_and_shell_mutations() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "before").unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let approvals = Arc::new(AtomicUsize::new(0));
    let make_ctx = || {
        let cancel_on_approval = Arc::clone(&cancel);
        let approvals_seen = Arc::clone(&approvals);
        ToolCtx::new(
            dir.path().to_path_buf(),
            Box::new(move |_| {
                approvals_seen.fetch_add(1, Ordering::SeqCst);
                cancel_on_approval.store(true, Ordering::Release);
                true
            }),
            Box::new(|access| match access {
                Access::Write(_) | Access::Exec(_) => Guard::Ask,
                _ => Guard::Allow,
            }),
            empty_test_scrubber(),
        )
        .with_cancel(Arc::clone(&cancel))
    };

    let created = WriteFile
        .execute(
            json!({"path": "new.txt", "content": "must not exist"}),
            &make_ctx(),
        )
        .unwrap();
    assert_eq!(created, "turn cancelled before file creation");
    assert!(!dir.path().join("new.txt").exists());

    cancel.store(false, Ordering::Release);
    let edited = EditFile
        .execute(
            json!({"path": "note.txt", "old_string": "before", "new_string": "after"}),
            &make_ctx(),
        )
        .unwrap();
    assert_eq!(edited, "turn cancelled before file edit");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("note.txt")).unwrap(),
        "before"
    );

    cancel.store(false, Ordering::Release);
    let executed = ExecShell
        .execute_with_audit(
            json!({"command": "echo should-not-run > marker.txt"}),
            &make_ctx(),
        )
        .unwrap();
    assert_eq!(executed.output, "turn cancelled before command execution");
    assert_eq!(
        executed.audit,
        vec![ToolAudit::Command(CommandOutcome::CancelledBeforeStart)]
    );
    assert!(!dir.path().join("marker.txt").exists());
    assert_eq!(approvals.load(Ordering::SeqCst), 3);
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
        let name = entry.unwrap().file_name();
        let name = name.to_string_lossy();
        !name.starts_with(".nh-write-") && !name.starts_with(".nh-edit-")
    }));
}

#[test]
fn write_file_labels_name_real_destination_and_requested_directory_alias() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    if symlink_dir(&real, &dir.path().join("alias")).is_err() {
        return;
    }
    let actions = Arc::new(std::sync::Mutex::new(Vec::new()));
    let actions_seen = Arc::clone(&actions);
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |action| {
            actions_seen.lock().unwrap().push(action.to_owned());
            true
        }),
        Box::new(|access| match access {
            Access::Write(_) => Guard::Ask,
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );

    let result = WriteFile
        .execute(json!({"path": "alias/new.txt", "content": "hello"}), &ctx)
        .unwrap();

    assert_eq!(
        *actions.lock().unwrap(),
        ["create real/new.txt (requested as alias/new.txt)"]
    );
    assert_eq!(
        result,
        "created real/new.txt (requested as alias/new.txt) (5 bytes)"
    );
    assert_eq!(
        std::fs::read_to_string(real.join("new.txt")).unwrap(),
        "hello"
    );
}

#[test]
fn write_file_approval_omits_requested_clause_for_real_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("dir")).unwrap();
    let actions = Arc::new(std::sync::Mutex::new(Vec::new()));
    let actions_seen = Arc::clone(&actions);
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |action| {
            actions_seen.lock().unwrap().push(action.to_owned());
            true
        }),
        Box::new(|access| match access {
            Access::Write(_) => Guard::Ask,
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );

    let result = WriteFile
        .execute(json!({"path": "dir/new.txt", "content": "hello"}), &ctx)
        .unwrap();

    assert_eq!(*actions.lock().unwrap(), ["create dir/new.txt"]);
    assert!(!actions.lock().unwrap()[0].contains("requested as"));
    assert_eq!(result, "created dir/new.txt (5 bytes)");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("dir/new.txt")).unwrap(),
        "hello"
    );
}

#[test]
fn oversized_write_is_refused_without_a_file_or_temp_artifact() {
    let dir = tempfile::tempdir().unwrap();
    let content = "x".repeat(MAX_TOOL_READ_BYTES + 1);

    let error = WriteFile
        .execute(
            json!({"path": "large.txt", "content": content}),
            &ctx_with(dir.path(), true),
        )
        .unwrap_err()
        .to_string();

    assert_eq!(
        error,
        format!("content too large to write safely (> {MAX_TOOL_READ_BYTES} bytes)")
    );
    assert!(!dir.path().join("large.txt").exists());
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".nh-write-")
    }));
}

#[test]
fn glob_files_returns_sorted_workdir_relative_paths() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src/nested")).unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("src/z.rs"), "").unwrap();
    std::fs::write(dir.path().join("src/a.rs"), "").unwrap();
    std::fs::write(dir.path().join("src/nested/m.rs"), "").unwrap();
    std::fs::write(dir.path().join("docs/ignored.rs"), "").unwrap();

    let result = GlobFiles
        .execute(
            json!({"pattern": "src/**/*.rs"}),
            &ctx_with(dir.path(), true),
        )
        .unwrap();
    let lines: Vec<&str> = result.lines().collect();

    assert_eq!(&lines[..3], ["src/a.rs", "src/nested/m.rs", "src/z.rs"]);
    assert!(lines[3].starts_with("- 3 matches; 4 files visited;"));
    assert!(lines[3].contains("0 files excluded by law"));
    assert!(lines[3].contains("0 symlinks skipped"));
}

#[test]
fn glob_files_refuses_scope_escape() {
    let dir = tempfile::tempdir().unwrap();
    let workdir = dir.path().join("inner");
    std::fs::create_dir(&workdir).unwrap();
    std::fs::write(dir.path().join("outside.txt"), "outside").unwrap();

    let error = GlobFiles
        .execute(
            json!({"pattern": "**", "path": ".."}),
            &ctx_with(&workdir, true),
        )
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("escapes the working directory"),
        "got: {error}"
    );
}

#[test]
fn glob_files_excludes_blocked_and_ask_files_without_approval() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["allowed.txt", "asked.txt", "blocked.txt"] {
        std::fs::write(dir.path().join(name), name).unwrap();
    }
    let approvals = Arc::new(AtomicUsize::new(0));
    let approvals_seen = Arc::clone(&approvals);
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |_| {
            approvals_seen.fetch_add(1, Ordering::SeqCst);
            true
        }),
        Box::new(|access| match access {
            Access::Read("asked.txt") => Guard::Ask,
            Access::Read("blocked.txt") => Guard::Block("protected".into()),
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );

    let result = GlobFiles.execute(json!({"pattern": "**"}), &ctx).unwrap();
    let lines: Vec<&str> = result.lines().collect();

    assert_eq!(lines[0], "allowed.txt");
    assert!(lines[1].starts_with("- 1 matches; 3 files visited;"));
    assert!(lines[1].contains("2 files excluded by law"));
    assert_eq!(approvals.load(Ordering::SeqCst), 0);
}

#[test]
fn glob_files_discloses_law_and_default_directory_pruning() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    std::fs::create_dir(dir.path().join("target")).unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join(".git/config"), "hidden").unwrap();
    std::fs::write(dir.path().join("target/cache"), "hidden").unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "visible").unwrap();
    let ctx = ctx_with(dir.path(), true).with_guard(Box::new(|access| match access {
        Access::Read(path) if nh_law::glob_matches(".git/**", path) => {
            Guard::Block("protected".into())
        }
        _ => Guard::Allow,
    }));

    let result = GlobFiles.execute(json!({"pattern": "**"}), &ctx).unwrap();

    assert!(result.starts_with("src/lib.rs\n- 1 matches;"));
    assert!(result.contains("1 directories pruned by law"));
    assert!(result.contains(
        "1 build/vendor directories pruned by default (target, node_modules, .venv, dist, build)"
    ));
    assert!(!result.contains(".git/config"));
    assert!(!result.contains("target/cache"));
}

#[test]
fn glob_files_stops_at_the_match_cap_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    const MATCH_CAP: usize = 500;
    for index in 0..=MATCH_CAP {
        std::fs::write(dir.path().join(format!("f{index:03}.rs")), "").unwrap();
    }

    let result = GlobFiles
        .execute(json!({"pattern": "*.rs"}), &ctx_with(dir.path(), true))
        .unwrap();
    let lines: Vec<&str> = result.lines().collect();

    assert_eq!(lines.len(), MATCH_CAP + 1);
    assert_eq!(lines[0], "f000.rs");
    assert_eq!(lines[MATCH_CAP - 1], "f499.rs");
    assert!(lines[MATCH_CAP].contains("stopped after the 500-match cap"));
}

#[test]
fn glob_files_file_visit_cap_is_honest() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(dir.path().join(name), "").unwrap();
    }

    let result = super::search::glob_files_with_limits(
        &json!({"pattern": "*.rs"}),
        &ctx_with(dir.path(), true),
        super::search::SearchLimits {
            files: 2,
            matches: 10,
        },
    )
    .unwrap();

    assert!(result.starts_with("- 0 matches; 2 files visited;"));
    assert!(result.contains("stopped after the 2-file visit cap"));
}

#[test]
fn grep_files_finds_literal_matches_with_glob_and_case_controls() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/a.rs"), "Alpha needle\nbeta NEEDLE\n").unwrap();
    std::fs::write(dir.path().join("src/z.rs"), "needle last\n").unwrap();
    std::fs::write(dir.path().join("src/ignored.txt"), "needle ignored\n").unwrap();

    let result = GrepFiles
        .execute(
            json!({
                "pattern": "needle",
                "path": "src",
                "glob": "**/*.rs",
                "case_insensitive": true
            }),
            &ctx_with(dir.path(), true),
        )
        .unwrap();
    let lines: Vec<&str> = result.lines().collect();

    assert_eq!(
        &lines[..3],
        [
            "src/a.rs:1:Alpha needle",
            "src/a.rs:2:beta NEEDLE",
            "src/z.rs:1:needle last"
        ]
    );
    assert!(lines[3].starts_with("- 3 matches in 2 files; 3 files visited;"));
    assert!(lines[3].contains("0 files excluded by law"));
    assert!(lines[3].contains("0 binary files skipped"));
}

#[test]
fn grep_files_treats_regex_syntax_as_literal_text() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("values.txt"),
        "number 123\nliteral \\\\d+\n",
    )
    .unwrap();

    let result = GrepFiles
        .execute(json!({"pattern": "\\d+"}), &ctx_with(dir.path(), true))
        .unwrap();

    assert!(result.starts_with("values.txt:2:literal \\\\d+\n- 1 matches"));
    assert!(!result.contains("number 123"));
}

#[test]
fn grep_files_refuses_scope_escape() {
    let dir = tempfile::tempdir().unwrap();
    let workdir = dir.path().join("inner");
    std::fs::create_dir(&workdir).unwrap();
    std::fs::write(dir.path().join("outside.txt"), "needle").unwrap();

    let error = GrepFiles
        .execute(
            json!({"pattern": "needle", "path": ".."}),
            &ctx_with(&workdir, true),
        )
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("escapes the working directory"),
        "got: {error}"
    );
}

#[test]
fn grep_files_excludes_law_files_without_prompting_and_discloses_pruning() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["allowed.txt", "asked.txt", "blocked.txt"] {
        std::fs::write(dir.path().join(name), "needle").unwrap();
    }
    std::fs::create_dir(dir.path().join("target")).unwrap();
    std::fs::write(dir.path().join("target/hidden.txt"), "needle").unwrap();
    let approvals = Arc::new(AtomicUsize::new(0));
    let approvals_seen = Arc::clone(&approvals);
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |_| {
            approvals_seen.fetch_add(1, Ordering::SeqCst);
            true
        }),
        Box::new(|access| match access {
            Access::Read("asked.txt") => Guard::Ask,
            Access::Read("blocked.txt") => Guard::Block("protected".into()),
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );

    let result = GrepFiles
        .execute(json!({"pattern": "needle"}), &ctx)
        .unwrap();

    assert!(result.starts_with("allowed.txt:1:needle\n- 1 matches in 1 files;"));
    assert!(result.contains("2 files excluded by law"));
    assert!(result.contains(
        "1 build/vendor directories pruned by default (target, node_modules, .venv, dist, build)"
    ));
    assert!(!result.contains("asked.txt"));
    assert!(!result.contains("blocked.txt"));
    assert!(!result.contains("hidden.txt"));
    assert_eq!(approvals.load(Ordering::SeqCst), 0);
}

#[test]
fn grep_files_skips_binary_and_oversized_files_and_truncates_honestly() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("binary.bin"), b"needle\0hidden").unwrap();
    std::fs::write(
        dir.path().join("large.txt"),
        vec![b'x'; MAX_TOOL_READ_BYTES + 1],
    )
    .unwrap();
    std::fs::write(
        dir.path().join("long.txt"),
        format!("needle {}", "x".repeat(400)),
    )
    .unwrap();

    let result = GrepFiles
        .execute(json!({"pattern": "needle"}), &ctx_with(dir.path(), true))
        .unwrap();
    let lines: Vec<&str> = result.lines().collect();

    assert!(lines[0].starts_with("long.txt:1:needle "));
    assert!(lines[0].contains("…(+"), "got: {}", lines[0]);
    assert!(lines[0].ends_with(" more chars)"), "got: {}", lines[0]);
    assert!(lines[1].starts_with("- 1 matches in 1 files; 3 files visited;"));
    assert!(lines[1].contains("1 binary files skipped"));
    assert!(lines[1].contains("1 oversized files skipped"));
}

#[test]
fn grep_files_stops_at_the_match_cap_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    const MATCH_CAP: usize = 500;
    std::fs::write(
        dir.path().join("many.txt"),
        "needle\n".repeat(MATCH_CAP + 1),
    )
    .unwrap();

    let result = GrepFiles
        .execute(json!({"pattern": "needle"}), &ctx_with(dir.path(), true))
        .unwrap();
    let lines: Vec<&str> = result.lines().collect();

    assert_eq!(lines.len(), MATCH_CAP + 1);
    assert_eq!(lines[0], "many.txt:1:needle");
    assert_eq!(lines[MATCH_CAP - 1], "many.txt:500:needle");
    assert!(lines[MATCH_CAP].contains("stopped after the 500-match cap"));
}

#[test]
fn grep_files_file_visit_cap_is_honest() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(dir.path().join(name), "absent").unwrap();
    }

    let result = super::search::grep_files_with_limits(
        &json!({"pattern": "needle"}),
        &ctx_with(dir.path(), true),
        super::search::SearchLimits {
            files: 2,
            matches: 10,
        },
    )
    .unwrap();

    assert!(result.starts_with("- 0 matches in 0 files; 2 files visited;"));
    assert!(result.contains("stopped after the 2-file visit cap"));
}

#[test]
fn grep_files_scrubs_literal_secrets_before_returning_match_lines() {
    let dir = tempfile::tempdir().unwrap();
    const SECRET: &str = "fixture-literal-search-secret";
    std::fs::write(
        dir.path().join("secret.txt"),
        format!("needle {SECRET} suffix"),
    )
    .unwrap();
    let ctx =
        ctx_with(dir.path(), true).with_scrubber(nh_vault::Scrubber::new(vec![SECRET.to_owned()]));

    let result = GrepFiles
        .execute(json!({"pattern": "needle"}), &ctx)
        .unwrap();

    assert!(result.contains("needle [REDACTED] suffix"));
    assert!(!result.contains(SECRET));
}

#[test]
fn read_edit_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "hello old world").unwrap();
    let ctx = ctx_with(dir.path(), true);

    let text = ReadFile.execute(json!({"path": "note.txt"}), &ctx).unwrap();
    assert_eq!(text, "hello old world");

    let result = EditFile
        .execute(
            json!({"path": "note.txt", "old_string": "old", "new_string": "new"}),
            &ctx,
        )
        .unwrap();
    assert_eq!(result, "edited note.txt");

    let text = ReadFile.execute(json!({"path": "note.txt"}), &ctx).unwrap();
    assert_eq!(text, "hello new world");
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".nh-edit-")
    }));
}

#[test]
fn ranged_read_is_operator_enabled_and_preserves_path_only_behavior() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "one\ntwo\nthree\n").unwrap();
    let ctx = ctx_with(dir.path(), true);

    let mut default_tools = read_only_tools();
    let default_spec = default_tools.remove(0).spec();
    let direct_spec = ReadFile.spec();
    assert_eq!(default_spec.name, direct_spec.name);
    assert_eq!(default_spec.description, direct_spec.description);
    assert_eq!(default_spec.parameters, direct_spec.parameters);
    assert!(default_spec.parameters["properties"]["start_line"].is_null());
    assert_eq!(
        default_tools.len(),
        2,
        "default registry must retain the other read-only tools"
    );

    let disabled = ReadFile
        .execute(
            json!({"path": "note.txt", "start_line": 2, "line_count": 1}),
            &ctx,
        )
        .unwrap_err()
        .to_string();
    assert!(disabled.contains("operator must pass --enable-ranged-reads"));

    let mut enabled_tools = read_only_tools_with_features(ToolFeatures {
        ranged_reads: true,
        ..ToolFeatures::default()
    });
    let ranged = enabled_tools.remove(0);
    let ranged_spec = ranged.spec();
    assert_eq!(ranged_spec.name, "read_file");
    assert_eq!(ranged_spec.parameters["required"], json!(["path"]));
    assert_eq!(
        ranged_spec.parameters["properties"]["start_line"]["maximum"],
        MAX_RANGED_START_LINE
    );
    assert_eq!(
        ranged_spec.parameters["properties"]["line_count"]["maximum"],
        MAX_RANGED_LINE_COUNT
    );
    assert_eq!(
        ranged.execute(json!({"path": "note.txt"}), &ctx).unwrap(),
        ReadFile.execute(json!({"path": "note.txt"}), &ctx).unwrap()
    );
}

#[test]
fn ranged_read_returns_requested_unicode_crlf_lines_and_truthful_eof() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("note.txt"),
        "one\r\ncaf\u{e9}\r\n\u{4f60}\u{597d}\r\nfour\r\n",
    )
    .unwrap();
    let ctx = ctx_with(dir.path(), true);

    assert_eq!(
        RangedReadFile
            .execute(
                json!({"path": "note.txt", "start_line": 2, "line_count": 2}),
                &ctx,
            )
            .unwrap(),
        "lines 2-3 of note.txt:\ncaf\u{e9}\r\n\u{4f60}\u{597d}\r\n"
    );
    assert_eq!(
        RangedReadFile
            .execute(
                json!({"path": "note.txt", "start_line": 3, "line_count": 5}),
                &ctx,
            )
            .unwrap(),
        "lines 3-4 of note.txt (EOF reached):\n\u{4f60}\u{597d}\r\nfour\r\n"
    );

    let error = RangedReadFile
        .execute(
            json!({"path": "note.txt", "start_line": 5, "line_count": 1}),
            &ctx,
        )
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "start_line 5 is past end of file (4 lines): note.txt"
    );
}

#[test]
fn ranged_read_rejects_incomplete_invalid_and_excessive_ranges() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "one\ntwo\n").unwrap();
    let ctx = ctx_with(dir.path(), true);
    let cases = [
        (
            json!({"path": "note.txt", "start_line": 1}),
            "start_line and line_count must be supplied together",
        ),
        (
            json!({"path": "note.txt", "line_count": 1}),
            "start_line and line_count must be supplied together",
        ),
        (
            json!({"path": "note.txt", "start_line": 0, "line_count": 1}),
            "start_line must be a positive whole number",
        ),
        (
            json!({"path": "note.txt", "start_line": 1, "line_count": -1}),
            "line_count must be a positive whole number",
        ),
        (
            json!({"path": "note.txt", "start_line": "1", "line_count": 1}),
            "start_line must be a positive whole number",
        ),
        (
            json!({"path": "note.txt", "start_line": MAX_RANGED_START_LINE + 1, "line_count": 1}),
            "start_line exceeds the 100000-line scan limit",
        ),
        (
            json!({"path": "note.txt", "start_line": 1, "line_count": MAX_RANGED_LINE_COUNT + 1}),
            "line_count exceeds the 1000-line result limit",
        ),
    ];

    for (args, expected) in cases {
        let error = RangedReadFile.execute(args, &ctx).unwrap_err().to_string();
        assert_eq!(error, expected);
    }
}

#[test]
fn ranged_read_rejects_binary_large_file_and_oversized_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("binary.dat"), b"text\0payload").unwrap();
    let large = std::fs::File::create(dir.path().join("large.txt")).unwrap();
    large.set_len(MAX_RANGED_FILE_BYTES + 1).unwrap();
    std::fs::write(
        dir.path().join("long-line.txt"),
        vec![b'x'; MAX_RANGED_LINE_BYTES + 1],
    )
    .unwrap();
    let ctx = ctx_with(dir.path(), true);

    let cases = [
        ("binary.dat", "file looks binary"),
        ("large.txt", "file is too large for a ranged read"),
        (
            "long-line.txt",
            "line 1 exceeds the 65536-byte ranged-read limit",
        ),
    ];
    for (path, expected) in cases {
        let error = RangedReadFile
            .execute(
                json!({"path": path, "start_line": 1, "line_count": 1}),
                &ctx,
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "got: {error}");
    }
}

#[test]
fn ranged_read_preserves_guard_denial_and_cancellation_boundaries() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("protected.txt"), "private\n").unwrap();
    let approvals = Arc::new(AtomicUsize::new(0));
    let approvals_seen = Arc::clone(&approvals);
    let blocked_ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |_| {
            approvals_seen.fetch_add(1, Ordering::SeqCst);
            true
        }),
        Box::new(|access| match access {
            Access::Read("protected.txt") => Guard::Block("protected fixture".into()),
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );
    assert_eq!(
        RangedReadFile
            .execute(
                json!({"path": "protected.txt", "start_line": 1, "line_count": 1}),
                &blocked_ctx,
            )
            .unwrap(),
        "blocked by law: protected fixture"
    );
    assert_eq!(approvals.load(Ordering::SeqCst), 0);

    let denied_ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(|action| {
            assert_eq!(action, "read protected.txt");
            false
        }),
        Box::new(|_| Guard::Ask),
        empty_test_scrubber(),
    );
    assert_eq!(
        RangedReadFile
            .execute(
                json!({"path": "protected.txt", "start_line": 1, "line_count": 1}),
                &denied_ctx,
            )
            .unwrap(),
        "user denied: read protected.txt"
    );

    let cancelled = Arc::new(AtomicBool::new(true));
    let cancelled_ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(|_| panic!("cancelled reads must not ask for approval")),
        Box::new(|_| panic!("cancelled reads must not consult the guard")),
        empty_test_scrubber(),
    )
    .with_cancel(cancelled);
    assert_eq!(
        RangedReadFile
            .execute(
                json!({"path": "protected.txt", "start_line": 1, "line_count": 1}),
                &cancelled_ctx,
            )
            .unwrap(),
        "turn cancelled before file read"
    );

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel_during_approval = Arc::clone(&cancelled);
    let after_approval_ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |_| {
            cancel_during_approval.store(true, Ordering::Release);
            true
        }),
        Box::new(|_| Guard::Ask),
        empty_test_scrubber(),
    )
    .with_cancel(cancelled);
    assert_eq!(
        RangedReadFile
            .execute(
                json!({"path": "protected.txt", "start_line": 1, "line_count": 1}),
                &after_approval_ctx,
            )
            .unwrap(),
        "turn cancelled before file read"
    );
}

#[test]
fn ranged_read_rejects_symlink_escape() {
    let dir = tempfile::tempdir().unwrap();
    let workdir = dir.path().join("project");
    std::fs::create_dir(&workdir).unwrap();
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "outside\n").unwrap();
    if let Err(error) = symlink_file(&outside, &workdir.join("alias.txt")) {
        if matches!(
            error.kind(),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
        ) {
            eprintln!("skipping symlink fixture: {error}");
            return;
        }
        panic!("could not create symlink fixture: {error}");
    }

    let error = RangedReadFile
        .execute(
            json!({"path": "alias.txt", "start_line": 1, "line_count": 1}),
            &ctx_with(&workdir, true),
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("escapes the working directory"),
        "got: {error}"
    );
}

#[test]
fn retained_observation_recovers_middle_without_recursive_capture_and_cleans_up() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("runtime");
    let content = format!(
        "HEAD\n{}\nMIDDLE-EVIDENCE\n{}\nTAIL",
        "a".repeat(18_000),
        "b".repeat(18_000)
    );
    std::fs::write(dir.path().join("large.txt"), &content).unwrap();
    let (ctx, session) = observation_context(
        dir.path(),
        &runtime,
        true,
        nh_vault::Scrubber::new(Vec::new()),
    );

    let inline = ReadFile
        .execute(json!({"path": "large.txt"}), &ctx)
        .unwrap();
    assert!(inline.contains("observation retained: handle=obs_"));
    assert!(
        inline.contains(&format!("scrubbed_bytes={}", content.len())),
        "got: {inline}"
    );
    assert!(inline.contains("source tool limits may already have truncated this result"));
    assert!(!inline.contains("MIDDLE-EVIDENCE"));
    let handle = observation_handle(&inline);
    assert_eq!(handle.len(), 36);
    assert!(handle[4..].bytes().all(|byte| byte.is_ascii_hexdigit()));
    let before = std::fs::read_dir(
        std::fs::read_dir(&runtime)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path(),
    )
    .unwrap()
    .count();
    let offset = u64::try_from(content.find("MIDDLE-EVIDENCE").unwrap()).unwrap();
    let recovered = observation_tool(&session)
        .execute(
            json!({
                "handle": &handle,
                "char_offset": offset,
                "char_count": 15
            }),
            &ctx,
        )
        .unwrap();
    assert!(recovered.ends_with("\nMIDDLE-EVIDENCE"), "got: {recovered}");
    let after = std::fs::read_dir(
        std::fs::read_dir(&runtime)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path(),
    )
    .unwrap()
    .count();
    assert_eq!(
        after, before,
        "retrieval must not recursively retain itself"
    );

    session.cleanup().unwrap();
    assert_eq!(std::fs::read_dir(runtime).unwrap().count(), 0);
}

#[test]
fn retained_observation_scrubs_before_persisting_and_rejects_tampering() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("runtime");
    const SECRET: &str = "fixture-literal-observation-value";
    let content = format!("{}\n{SECRET}\n{}", "a".repeat(17_000), "b".repeat(17_000));
    let (ctx, session) = observation_context(
        dir.path(),
        &runtime,
        true,
        nh_vault::Scrubber::new(vec![SECRET.to_owned()]),
    );

    let inline = render_tool_result(content, &ctx);
    let handle = observation_handle(&inline);
    let path = only_observation_file(&runtime);
    let stored = std::fs::read_to_string(&path).unwrap();
    assert!(!stored.contains(SECRET));
    assert!(stored.contains("[REDACTED]"));
    let mut tampered = stored.into_bytes();
    tampered[0] = if tampered[0] == b'x' { b'y' } else { b'x' };
    std::fs::write(&path, tampered).unwrap();

    let error = observation_tool(&session)
        .execute(
            json!({"handle": &handle, "char_offset": 0, "char_count": 20}),
            &ctx,
        )
        .unwrap_err()
        .to_string();
    assert_eq!(error, "retained observation failed digest verification");
}

#[test]
fn observation_character_ranges_are_unicode_safe_bounded_and_cancellable() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("runtime");
    let content = format!("{}END \u{1f642}\u{e9}\u{6f22}", "x".repeat(33_000));
    let total_chars = u64::try_from(content.chars().count()).unwrap();
    let (ctx, session) = observation_context(
        dir.path(),
        &runtime,
        true,
        nh_vault::Scrubber::new(Vec::new()),
    );
    let inline = render_tool_result(content, &ctx);
    let handle = observation_handle(&inline);
    let tool = observation_tool(&session);

    let recovered = tool
        .execute(
            json!({
                "handle": &handle,
                "char_offset": total_chars - 3,
                "char_count": 10
            }),
            &ctx,
        )
        .unwrap();
    assert!(recovered.contains("(end reached):\n\u{1f642}\u{e9}\u{6f22}"));
    for (args, expected) in [
        (
            json!({"handle": &handle, "char_offset": total_chars, "char_count": 1}),
            "is at or past end of observation",
        ),
        (
            json!({"handle": &handle, "char_offset": 0, "char_count": 0}),
            "char_count must be a positive whole number",
        ),
        (
            json!({"handle": &handle, "char_offset": -1, "char_count": 1}),
            "char_offset must be a non-negative whole number",
        ),
    ] {
        let error = tool.execute(args, &ctx).unwrap_err().to_string();
        assert!(error.contains(expected), "got: {error}");
    }

    let cancelled = Arc::new(AtomicBool::new(true));
    let cancelled_ctx = ctx.with_cancel(cancelled);
    assert_eq!(
        tool.execute(
            json!({"handle": &handle, "char_offset": 0, "char_count": 1}),
            &cancelled_ctx,
        )
        .unwrap(),
        "turn cancelled before observation read"
    );
}

#[test]
fn observation_handles_are_exact_session_and_boundary_capabilities() {
    let dir = tempfile::tempdir().unwrap();
    let runtime_a = dir.path().join("runtime-a");
    let runtime_b = dir.path().join("runtime-b");
    let (ctx_a, session_a) = observation_context(
        dir.path(),
        &runtime_a,
        true,
        nh_vault::Scrubber::new(Vec::new()),
    );
    let (ctx_b, session_b) = observation_context(
        dir.path(),
        &runtime_b,
        true,
        nh_vault::Scrubber::new(Vec::new()),
    );
    let inline = render_tool_result("x".repeat(33_000), &ctx_a);
    let handle = observation_handle(&inline);
    let tool_a = observation_tool(&session_a);
    let tool_b = observation_tool(&session_b);

    let fake = tool_a
        .execute(
            json!({"handle": "../../catalog.toml", "char_offset": 0, "char_count": 1}),
            &ctx_a,
        )
        .unwrap_err()
        .to_string();
    assert_eq!(fake, "unknown observation handle for this session");
    let crossed = tool_b
        .execute(
            json!({"handle": &handle, "char_offset": 0, "char_count": 1}),
            &ctx_b,
        )
        .unwrap_err()
        .to_string();
    assert_eq!(crossed, "unknown observation handle for this session");
    let wrong_boundary = tool_a
        .execute(
            json!({"handle": &handle, "char_offset": 0, "char_count": 1}),
            &ctx_b,
        )
        .unwrap_err()
        .to_string();
    assert_eq!(
        wrong_boundary,
        "observation handle is not valid for this tool boundary"
    );
    let changed_ctx = ctx_a.with_guard(permissive_test_guard());
    let changed_boundary = tool_a
        .execute(
            json!({"handle": &handle, "char_offset": 0, "char_count": 1}),
            &changed_ctx,
        )
        .unwrap_err()
        .to_string();
    assert_eq!(
        changed_boundary,
        "observation handle is not valid for this tool boundary"
    );
}

#[test]
fn retained_observation_rejects_final_symlink_without_reading_outside() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("runtime");
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, "outside sentinel").unwrap();
    let (ctx, session) = observation_context(
        dir.path(),
        &runtime,
        true,
        nh_vault::Scrubber::new(Vec::new()),
    );
    let inline = render_tool_result("x".repeat(33_000), &ctx);
    let handle = observation_handle(&inline);
    let path = only_observation_file(&runtime);
    std::fs::remove_file(&path).unwrap();
    if let Err(error) = symlink_file(&outside, &path) {
        if matches!(
            error.kind(),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
        ) {
            eprintln!("skipping symlink fixture: {error}");
            return;
        }
        panic!("could not create symlink fixture: {error}");
    }

    let error = observation_tool(&session)
        .execute(
            json!({"handle": &handle, "char_offset": 0, "char_count": 1}),
            &ctx,
        )
        .unwrap_err()
        .to_string();
    assert_eq!(error, "retained observation path is not a regular file");
    assert_eq!(
        std::fs::read_to_string(outside).unwrap(),
        "outside sentinel"
    );
}

#[test]
fn observation_mode_keeps_denied_shell_from_running_or_creating_a_record() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = dir.path().join("runtime");
    let marker = dir.path().join("must-not-exist.txt");
    let (ctx, _session) = observation_context(
        dir.path(),
        &runtime,
        false,
        nh_vault::Scrubber::new(Vec::new()),
    );
    let command = if cfg!(windows) {
        "echo unexpected>must-not-exist.txt"
    } else {
        "echo unexpected > must-not-exist.txt"
    };

    let output = ExecShell
        .execute(json!({"command": command}), &ctx)
        .unwrap();

    assert_eq!(output, format!("user denied: {command}"));
    assert!(!marker.exists());
    let session_dir = std::fs::read_dir(runtime)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(std::fs::read_dir(session_dir).unwrap().count(), 0);
}

#[test]
fn edit_file_success_names_canonical_path_and_requested_directory_alias() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::fs::write(real.join("note.txt"), "before").unwrap();
    if symlink_dir(&real, &dir.path().join("alias")).is_err() {
        return;
    }
    let actions = Arc::new(std::sync::Mutex::new(Vec::new()));
    let actions_seen = Arc::clone(&actions);
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |action| {
            actions_seen.lock().unwrap().push(action.to_owned());
            true
        }),
        Box::new(|access| match access {
            Access::Write(_) => Guard::Ask,
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );

    let result = EditFile
        .execute(
            json!({"path": "alias/note.txt", "old_string": "before", "new_string": "after"}),
            &ctx,
        )
        .unwrap();

    assert_eq!(
        *actions.lock().unwrap(),
        ["edit real/note.txt (requested as alias/note.txt)"]
    );
    assert_eq!(result, "edited real/note.txt (requested as alias/note.txt)");
    assert_eq!(
        std::fs::read_to_string(real.join("note.txt")).unwrap(),
        "after"
    );
}

#[test]
fn edit_file_success_omits_requested_clause_for_ordinary_path() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "before").unwrap();

    let result = EditFile
        .execute(
            json!({"path": "note.txt", "old_string": "before", "new_string": "after"}),
            &ctx_with(dir.path(), true),
        )
        .unwrap();

    assert_eq!(result, "edited note.txt");
    assert!(!result.contains("(requested as"));
}

#[test]
fn read_uses_session_scrubber_for_literal_secrets() {
    let dir = tempfile::tempdir().unwrap();
    const LITERAL: &str = "fixture-literal-abc123";
    std::fs::write(dir.path().join("secret.txt"), LITERAL).unwrap();
    let ctx = ctx_with(dir.path(), true)
        .with_scrubber(nh_vault::Scrubber::new(vec![LITERAL.to_string()]));

    let result = ReadFile
        .execute(json!({"path": "secret.txt"}), &ctx)
        .unwrap();

    assert_eq!(result, "[REDACTED]");
    assert!(!result.contains(LITERAL));
}

#[test]
fn explicit_empty_scrubber_still_scrubs_key_shapes() {
    let dir = tempfile::tempdir().unwrap();
    let shaped = "sk-fixture-abc123";
    let plain = "fixture-literal-abc123";
    std::fs::write(dir.path().join("output.txt"), format!("{shaped}\n{plain}")).unwrap();

    let result = ReadFile
        .execute(json!({"path": "output.txt"}), &ctx_with(dir.path(), true))
        .unwrap();

    assert_eq!(result, format!("[REDACTED]\n{plain}"));
}

#[test]
fn read_refuses_file_with_nul_prefix_and_names_path() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("binary.bin"), b"text\0payload").unwrap();

    let error = ReadFile
        .execute(json!({"path": "binary.bin"}), &ctx_with(dir.path(), true))
        .unwrap_err()
        .to_string();

    assert_eq!(
        error,
        "file looks binary: binary.bin - choose a text file or use a binary-aware tool"
    );
}

#[test]
fn read_preserves_valid_utf8_output() {
    let dir = tempfile::tempdir().unwrap();
    let expected = "plain text\ncaf\u{e9}\n\u{4f60}\u{597d}";
    std::fs::write(dir.path().join("valid.txt"), expected).unwrap();

    let result = ReadFile
        .execute(json!({"path": "valid.txt"}), &ctx_with(dir.path(), true))
        .unwrap();

    assert_eq!(result, expected);
}

#[test]
fn read_reports_lossy_utf8_decoding() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("latin-1.txt"),
        vec![b'c', b'a', b'f', 0xff, b'e'],
    )
    .unwrap();

    let result = ReadFile
        .execute(json!({"path": "latin-1.txt"}), &ctx_with(dir.path(), true))
        .unwrap();

    assert_eq!(
        result,
        "caf\u{fffd}e\n\u{2026}[some bytes were not valid UTF-8 and were replaced]"
    );
}

#[test]
fn truncated_valid_utf8_does_not_report_invalid_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = vec![b'x'; MAX_TOOL_READ_BYTES - 1];
    bytes.extend_from_slice("\u{e9}".as_bytes());
    bytes.push(b'z');
    assert!(std::str::from_utf8(&bytes).is_ok());
    std::fs::write(dir.path().join("large-utf8.txt"), bytes).unwrap();

    let result = ReadFile
        .execute(
            json!({"path": "large-utf8.txt"}),
            &ctx_with(dir.path(), true),
        )
        .unwrap();

    assert!(
        result.contains("input truncated at 2097152 bytes"),
        "got: {result}"
    );
    assert!(!result.contains("some bytes were not valid UTF-8"));
    assert!(!result.contains('\u{fffd}'));
}

#[test]
fn valid_replacement_character_does_not_report_invalid_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let expected = "before \u{fffd} after";
    std::fs::write(dir.path().join("replacement.txt"), expected).unwrap();

    let result = ReadFile
        .execute(
            json!({"path": "replacement.txt"}),
            &ctx_with(dir.path(), true),
        )
        .unwrap();

    assert_eq!(result, expected);
    assert!(!result.contains("some bytes were not valid UTF-8"));
}

#[test]
fn oversized_read_is_bounded_before_envelope_elision() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("large.txt"),
        vec![b'x'; MAX_TOOL_READ_BYTES + 100_000],
    )
    .unwrap();

    let result = ReadFile
        .execute(json!({"path": "large.txt"}), &ctx_with(dir.path(), true))
        .unwrap();

    assert!(result.contains("chars elided; digest "), "got: {result}");
    assert!(
        result.contains("input truncated at 2097152 bytes"),
        "got: {result}"
    );
    assert!(result.chars().count() <= MAX_TOOL_RESULT_CHARS + 100);
}

#[test]
fn read_missing_file_names_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_with(dir.path(), true);
    let err = ReadFile
        .execute(json!({"path": "nope.txt"}), &ctx)
        .unwrap_err()
        .to_string();
    assert!(err.contains("file not found: nope.txt"), "got: {err}");
}

#[test]
fn edit_old_string_not_found() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "first line\nabc\nlast line").unwrap();
    let ctx = ctx_with(dir.path(), true);
    let err = EditFile
        .execute(
            json!({"path": "a.txt", "old_string": "abd", "new_string": "y"}),
            &ctx,
        )
        .unwrap_err()
        .to_string();
    assert!(err.starts_with("old_string not found in a.txt\n"));
    assert!(err.contains("nearest candidate: a.txt:2-2"), "got: {err}");
    assert!(err.ends_with("actual text:\nabc"), "got: {err}");
}

#[test]
fn edit_non_unique_old_string() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "foo bar foo").unwrap();
    let ctx = ctx_with(dir.path(), true);
    let err = EditFile
        .execute(
            json!({"path": "a.txt", "old_string": "foo", "new_string": "baz"}),
            &ctx,
        )
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        "old_string appears 2 times in a.txt - provide more context"
    );
}

#[test]
fn edit_uses_and_audits_whitespace_normalized_match() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "let answer   =  41;\n").unwrap();

    let execution = EditFile
        .execute_with_audit(
            json!({
                "path": "a.txt",
                "old_string": "let answer = 41;",
                "new_string": "let answer = 42;"
            }),
            &ctx_with(dir.path(), true),
        )
        .unwrap();

    assert_eq!(
        execution.output,
        "edited a.txt using whitespace-normalized match"
    );
    assert_eq!(
        execution.audit,
        vec![
            ToolAudit::EditMatch(EditMatchTier::WhitespaceNormalized),
            ToolAudit::FilePublished(FileChangeKind::Edited),
        ]
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "let answer = 42;\n");
}

#[test]
fn edit_uses_and_audits_indentation_flexible_match() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "    if ready {\n        run();\n    }\n").unwrap();

    let execution = EditFile
        .execute_with_audit(
            json!({
                "path": "a.txt",
                "old_string": "if ready {\n  run();\n}",
                "new_string": "if ready {\n  finish();\n}"
            }),
            &ctx_with(dir.path(), true),
        )
        .unwrap();

    assert_eq!(
        execution.output,
        "edited a.txt using indentation-flexible match"
    );
    assert_eq!(
        execution.audit,
        vec![
            ToolAudit::EditMatch(EditMatchTier::IndentationFlexible),
            ToolAudit::FilePublished(FileChangeKind::Edited),
        ]
    );
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "    if ready {\n        finish();\n    }\n"
    );
}

#[test]
fn tolerant_match_ambiguity_fails_without_editing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    let original = "alpha   beta\nalpha  beta\n";
    std::fs::write(&path, original).unwrap();

    let error = EditFile
        .execute(
            json!({
                "path": "a.txt",
                "old_string": "alpha beta",
                "new_string": "changed"
            }),
            &ctx_with(dir.path(), true),
        )
        .unwrap_err()
        .to_string();

    assert_eq!(
        error,
        "old_string has 2 whitespace-normalized matches in a.txt - provide more context"
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), original);
}

#[test]
fn nearest_candidate_is_scrubbed_before_returning_to_the_model() {
    let dir = tempfile::tempdir().unwrap();
    const SECRET: &str = "fixture-secret-value";
    std::fs::write(dir.path().join("a.txt"), format!("prefix {SECRET} suffix")).unwrap();
    let ctx =
        ctx_with(dir.path(), true).with_scrubber(nh_vault::Scrubber::new(vec![SECRET.to_string()]));

    let error = EditFile
        .execute(
            json!({
                "path": "a.txt",
                "old_string": "prefix changed suffix",
                "new_string": "replacement"
            }),
            &ctx,
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("prefix [REDACTED] suffix"), "got: {error}");
    assert!(!error.contains(SECRET));
}

#[test]
fn oversized_edit_is_refused_and_leaves_the_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.txt");
    let original = vec![b'x'; MAX_TOOL_READ_BYTES + 1];
    std::fs::write(&path, &original).unwrap();

    let error = EditFile
        .execute(
            json!({"path": "large.txt", "old_string": "x", "new_string": "y"}),
            &ctx_with(dir.path(), true),
        )
        .unwrap_err()
        .to_string();

    assert_eq!(
        error,
        format!("file too large to edit safely (> {MAX_TOOL_READ_BYTES} bytes)")
    );
    assert_eq!(std::fs::read(path).unwrap(), original);
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".nh-edit-")
    }));
}

#[test]
fn path_escape_blocked_for_existing_and_missing_files() {
    let dir = tempfile::tempdir().unwrap();
    let workdir = dir.path().join("inner");
    std::fs::create_dir(&workdir).unwrap();
    std::fs::write(dir.path().join("secret.txt"), "sk-test-0000").unwrap();
    let ctx = ctx_with(&workdir, true);

    // Existing file above workdir (canonicalize branch).
    let err = ReadFile
        .execute(json!({"path": "../secret.txt"}), &ctx)
        .unwrap_err()
        .to_string();
    assert!(err.contains("escapes the working directory"), "got: {err}");

    // Missing file above workdir (lexical branch) - still an escape, not "not found".
    let err = ReadFile
        .execute(json!({"path": "../missing.txt"}), &ctx)
        .unwrap_err()
        .to_string();
    assert!(err.contains("escapes the working directory"), "got: {err}");

    // Absolute path outside workdir.
    let abs = dir.path().join("secret.txt").display().to_string();
    let err = ReadFile
        .execute(json!({"path": abs}), &ctx)
        .unwrap_err()
        .to_string();
    assert!(err.contains("escapes the working directory"), "got: {err}");

    // EditFile goes through the same gate.
    let err = EditFile
        .execute(
            json!({"path": "../secret.txt", "old_string": "a", "new_string": "b"}),
            &ctx,
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("escapes the working directory"), "got: {err}");
}

#[test]
fn exec_denied_never_runs_and_is_ok_shaped() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_seen = calls.clone();
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |cmd| {
            assert_eq!(cmd, "exec echo pwned > marker.txt");
            calls_seen.fetch_add(1, Ordering::SeqCst);
            false
        }),
        permissive_test_guard(),
        empty_test_scrubber(),
    );
    let execution = ExecShell
        .execute_with_audit(json!({"command": "echo pwned > marker.txt"}), &ctx)
        .unwrap();
    assert_eq!(execution.output, "user denied: echo pwned > marker.txt");
    assert_eq!(
        execution.audit,
        vec![ToolAudit::Command(CommandOutcome::Denied)]
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "approval gate must be consulted"
    );
    assert!(
        !dir.path().join("marker.txt").exists(),
        "denied command must never execute"
    );
}

#[test]
fn exec_blocked_fact_is_not_an_execution() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(|_| panic!("blocked command must not ask for approval")),
        Box::new(|access| match access {
            Access::Exec(_) => Guard::Block("blocked command".into()),
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );

    let execution = ExecShell
        .execute_with_audit(json!({"command": "echo must-not-run"}), &ctx)
        .unwrap();

    assert_eq!(execution.output, "blocked by law: blocked command");
    assert_eq!(
        execution.audit,
        vec![ToolAudit::Command(CommandOutcome::Blocked)]
    );
}

#[test]
fn exec_denial_scrubs_the_command_before_returning_it() {
    const LITERAL: &str = "header-value-fixture";
    let dir = tempfile::tempdir().unwrap();
    let command = format!("curl -H \"Authorization: Bearer {LITERAL}\" https://example.test");
    let expected_command = command.clone();
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |seen| {
            assert_eq!(seen, format!("exec {expected_command}"));
            false
        }),
        permissive_test_guard(),
        nh_vault::Scrubber::new(vec![LITERAL.to_string()]),
    );

    let result = ExecShell
        .execute(json!({"command": command}), &ctx)
        .unwrap();

    assert!(result.starts_with("user denied:"), "got: {result}");
    assert!(!result.contains(LITERAL), "literal leaked: {result}");
    assert!(result.contains("Bearer [REDACTED]"), "got: {result}");
}

#[test]
fn exec_guard_allow_still_requires_explicit_approval() {
    let dir = tempfile::tempdir().unwrap();
    let approvals = Arc::new(AtomicUsize::new(0));
    let approvals_seen = Arc::clone(&approvals);
    let command = "echo pwned > marker.txt";
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |seen_command| {
            assert_eq!(seen_command, format!("exec {command}"));
            approvals_seen.fetch_add(1, Ordering::SeqCst);
            false
        }),
        Box::new(|access| match access {
            Access::Exec(_) => Guard::Allow,
            _ => Guard::Block("unexpected access".into()),
        }),
        empty_test_scrubber(),
    );

    let result = ExecShell
        .execute_with_deadlines(
            json!({"command": command}),
            &ctx,
            Duration::from_millis(100),
            Duration::from_millis(50),
        )
        .unwrap();

    assert_eq!(result, format!("user denied: {command}"));
    assert_eq!(approvals.load(Ordering::SeqCst), 1);
    assert!(
        !dir.path().join("marker.txt").exists(),
        "unapproved command must never execute"
    );
}

#[test]
fn exec_echo_happy_path() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_with(dir.path(), true);
    let execution = ExecShell
        .execute_with_audit(json!({"command": "echo hello"}), &ctx)
        .unwrap();
    assert!(
        execution.output.contains("exit code: 0"),
        "got: {}",
        execution.output
    );
    assert!(
        execution.output.contains("hello"),
        "got: {}",
        execution.output
    );
    assert_eq!(
        execution.audit,
        vec![ToolAudit::Command(CommandOutcome::Exited(Some(0)))]
    );
}

#[cfg(windows)]
#[test]
fn exec_windows_preserves_approved_embedded_quotes_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_with(dir.path(), true);
    let command = r#"echo a "b c" d"#;

    let result = ExecShell
        .execute(json!({"command": command}), &ctx)
        .unwrap();

    assert!(result.contains("a \"b c\" d"), "got: {result}");
}

#[test]
fn exec_timeout_kills_child_and_prevents_late_marker() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_with(dir.path(), true);
    let command = if cfg!(windows) {
        "ping -n 6 127.0.0.1 > nul && echo late > marker.txt"
    } else {
        "sleep 5; echo late > marker.txt"
    };

    let started = Instant::now();
    let execution = ExecShell
        .execute_with_deadlines_audited(
            json!({"command": command}),
            &ctx,
            Duration::from_millis(100),
            Duration::from_millis(150),
        )
        .unwrap();
    let elapsed = started.elapsed();
    thread::sleep(Duration::from_millis(1_200));

    assert!(
        execution
            .output
            .contains("command timed out after 100ms - killed"),
        "got: {}",
        execution.output
    );
    assert_eq!(
        execution.audit,
        vec![ToolAudit::Command(CommandOutcome::TimedOut {
            termination_complete: true,
        })]
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "timeout waited for a descendant instead of killing its process tree: {elapsed:?}"
    );
    assert!(
        !dir.path().join("marker.txt").exists(),
        "timed-out command continued after its shell was killed"
    );
}

#[test]
fn exec_cancel_kills_child_without_claiming_a_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let ctx = ctx_with(dir.path(), true).with_cancel(Arc::clone(&cancel));
    let command = if cfg!(windows) {
        "ping -n 6 127.0.0.1 > nul && echo late > marker.txt"
    } else {
        "sleep 5; echo late > marker.txt"
    };
    let cancel_thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        cancel.store(true, Ordering::Release);
    });

    let started = Instant::now();
    let execution = ExecShell
        .execute_with_deadlines_audited(
            json!({"command": command}),
            &ctx,
            Duration::from_secs(5),
            Duration::from_millis(150),
        )
        .unwrap();
    let elapsed = started.elapsed();
    cancel_thread.join().unwrap();
    thread::sleep(Duration::from_millis(1_200));

    assert!(
        execution.output.contains("command cancelled - killed"),
        "got: {}",
        execution.output
    );
    assert!(
        !execution.output.contains("timed out"),
        "got: {}",
        execution.output
    );
    assert_eq!(
        execution.audit,
        vec![ToolAudit::Command(CommandOutcome::Cancelled {
            termination_complete: true,
        })]
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "cancel waited for a descendant instead of killing its process tree: {elapsed:?}"
    );
    assert!(
        !dir.path().join("marker.txt").exists(),
        "cancelled command continued after its shell was killed"
    );
}

#[test]
fn failed_tree_kill_is_reported_even_when_direct_child_is_reaped() {
    #[cfg(windows)]
    let mut child = std::process::Command::new("ping.exe")
        .args(["-n", "6", "127.0.0.1"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    #[cfg(not(windows))]
    let mut child = std::process::Command::new("sleep")
        .arg("5")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let termination = terminate_child_tree_with(
        &mut child,
        "injected tree-kill failure",
        TerminationReason::Cancelled,
        |_| (false, "tree-kill injected failure".into()),
    )
    .unwrap();

    let Termination::Incomplete { detail, .. } = termination else {
        panic!("failed tree kill must not be reported as complete");
    };
    assert!(
        detail.contains("tree-kill injected failure"),
        "got: {detail}"
    );
    assert!(
        detail.contains("direct child reaped, but descendant termination was not verified"),
        "got: {detail}"
    );
    assert!(child.try_wait().unwrap().is_some());
}

#[test]
fn bounded_drain_returns_partial_output_when_reader_stays_open() {
    let (blocked_tx, blocked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let drain = spawn_drain(BlockingReader {
        first: Some(b"partial output".to_vec()),
        blocked: Some(blocked_tx),
        release: release_rx,
    });
    blocked_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("reader reached its blocking read");

    let grace = Duration::from_millis(150);
    let started = Instant::now();
    let outcome = drain.finish(started + grace, grace);
    let elapsed = started.elapsed();
    let rendered = render_bounded_output(outcome, "stdout");
    drop(release_tx);

    assert!(
        elapsed < Duration::from_secs(1),
        "bounded drain exceeded its deadline: {elapsed:?}"
    );
    assert!(rendered.starts_with("partial output"), "got: {rendered}");
    assert!(
        rendered.contains(
            "…[stdout capture incomplete after 150ms - a surviving child process may still hold the pipe]"
        ),
        "got: {rendered}"
    );
}

#[test]
fn bounded_drain_completes_and_preserves_truncation_marker() {
    let grace = Duration::from_secs(2);
    let complete = spawn_drain(std::io::Cursor::new(b"all bytes".to_vec()))
        .finish(Instant::now() + grace, grace);
    assert!(matches!(&complete.completion, DrainCompletion::Complete));
    assert_eq!(render_bounded_output(complete, "stdout"), "all bytes");

    let truncated = spawn_drain(std::io::Cursor::new(vec![b'x'; MAX_TOOL_READ_BYTES + 1]))
        .finish(Instant::now() + grace, grace);
    assert!(truncated.output.truncated);
    assert!(matches!(&truncated.completion, DrainCompletion::Complete));
    let rendered = render_bounded_output(truncated, "stdout");
    assert!(rendered.ends_with(&format!(
        "\n…[stdout truncated at {MAX_TOOL_READ_BYTES} bytes]"
    )));
    assert!(!rendered.contains("capture incomplete"));
}

#[test]
fn truncated_incomplete_capture_renders_both_markers() {
    let rendered = render_bounded_output(
        DrainOutcome {
            output: BoundedOutput {
                bytes: b"partial".to_vec(),
                truncated: true,
            },
            completion: DrainCompletion::Incomplete(Duration::from_millis(150)),
        },
        "stdout",
    );

    assert_eq!(
        rendered,
        format!(
            "partial\n…[stdout truncated at {MAX_TOOL_READ_BYTES} bytes]\n…[stdout capture incomplete after 150ms - a surviving child process may still hold the pipe]"
        )
    );
}

#[test]
fn child_env_allowlist_is_case_insensitive_and_minimal() {
    assert!(is_allowed_env_var("PATH"));
    assert!(is_allowed_env_var("path"));
    assert!(is_allowed_env_var("CARGO_HOME"));
    assert!(!is_allowed_env_var("NH_DEEPSEEK_KEY"));
    assert!(!is_allowed_env_var("GITHUB_TOKEN"));
    assert!(!is_allowed_env_var("OPENAI_API_KEY"));
}

#[test]
fn exec_child_never_sees_nh_key_env_fallback() {
    std::env::set_var("NH_EXECTEST_KEY", "sk-test-0000-exec");
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_with(dir.path(), true);
    let command = if cfg!(windows) {
        "echo [%NH_EXECTEST_KEY%]"
    } else {
        "echo [${NH_EXECTEST_KEY:-unset}]"
    };
    let result = ExecShell
        .execute(json!({"command": command}), &ctx)
        .unwrap();
    std::env::remove_var("NH_EXECTEST_KEY");
    assert!(
        !result.contains("sk-test-0000-exec"),
        "child must not inherit NH_*_KEY: {result}"
    );
}

#[test]
fn exec_child_never_sees_ambient_github_token() {
    const SECRET: &str = "ambient-secret-must-not-pass";
    let previous = std::env::var_os("GITHUB_TOKEN");
    std::env::set_var("GITHUB_TOKEN", SECRET);
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_with(dir.path(), true);
    let command = if cfg!(windows) {
        "echo [%GITHUB_TOKEN%]"
    } else {
        "echo [${GITHUB_TOKEN:-unset}]"
    };
    let result = ExecShell
        .execute(json!({"command": command}), &ctx)
        .unwrap();
    match previous {
        Some(value) => std::env::set_var("GITHUB_TOKEN", value),
        None => std::env::remove_var("GITHUB_TOKEN"),
    }
    assert!(
        !result.contains(SECRET),
        "ambient credential leaked: {result}"
    );
}

#[test]
fn over_cap_exec_result_is_bounded_with_digest_marker() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_with(dir.path(), true);
    let command = if cfg!(windows) {
        "for /L %i in (1,1,40000) do @echo x"
    } else {
        "yes x | head -n 40000"
    };
    let raw_chars = 80_000;
    let result = ExecShell
        .execute(json!({"command": command}), &ctx)
        .unwrap();
    assert!(result.contains("chars elided; digest "), "got: {result}");
    assert!(result.chars().count() < raw_chars);
    assert!(result.chars().count() <= MAX_TOOL_RESULT_CHARS + 100);
}

#[test]
fn exec_stream_is_bounded_before_envelope_elision() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_with(dir.path(), true);
    let command = if cfg!(windows) {
        "powershell -NoProfile -Command \"[Console]::Out.Write('x' * 2200000)\""
    } else {
        "yes x | head -c 2200000"
    };

    let result = ExecShell
        .execute(json!({"command": command}), &ctx)
        .unwrap();

    assert!(result.contains("chars elided; digest "), "got: {result}");
    assert!(
        result.contains("stdout truncated at 2097152 bytes"),
        "got: {result}"
    );
    assert!(result.chars().count() <= MAX_TOOL_RESULT_CHARS + 100);
}

#[test]
fn exec_reports_nonzero_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_with(dir.path(), true);
    let execution = ExecShell
        .execute_with_audit(json!({"command": "exit 7"}), &ctx)
        .unwrap();
    assert!(
        execution.output.contains("exit code: 7"),
        "got: {}",
        execution.output
    );
    assert_eq!(
        execution.audit,
        vec![ToolAudit::Command(CommandOutcome::Exited(Some(7)))]
    );
}

#[test]
fn missing_argument_is_actionable() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ctx_with(dir.path(), true);
    let err = ReadFile.execute(json!({}), &ctx).unwrap_err().to_string();
    assert_eq!(err, "missing required argument: path");
}

#[test]
fn protected_edit_is_ok_shaped_and_leaves_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let protected = dir.path().join(".nosis").join("law.toml");
    std::fs::create_dir_all(protected.parent().unwrap()).unwrap();
    std::fs::write(&protected, "before").unwrap();
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(|_| true),
        Box::new(|access| match access {
            Access::Write(path) if *path == ".nosis/law.toml" => {
                Guard::Block("protected path (.nosis/**)".into())
            }
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );

    let execution = EditFile
        .execute_with_audit(
            json!({"path": ".nosis/law.toml", "old_string": "before", "new_string": "after"}),
            &ctx,
        )
        .unwrap();

    assert_eq!(
        execution.output,
        "blocked by law: protected path (.nosis/**)"
    );
    assert!(execution.audit.is_empty());
    assert_eq!(std::fs::read_to_string(protected).unwrap(), "before");
}

#[test]
fn protected_read_is_blocked_before_io_and_normal_source_is_allowed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src").join("lib.rs"), "pub fn safe() {}").unwrap();
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(|_| true),
        Box::new(|access| match access {
            Access::Read(".env") => Guard::Block("protected read (**/.env*)".into()),
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );

    let blocked = ReadFile.execute(json!({"path": ".env"}), &ctx).unwrap();
    assert_eq!(blocked, "blocked by law: protected read (**/.env*)");
    let allowed = ReadFile
        .execute(json!({"path": "src/lib.rs"}), &ctx)
        .unwrap();
    assert_eq!(allowed, "pub fn safe() {}");
}

#[test]
fn tool_result_redacts_key_shapes_before_egress() {
    let dir = tempfile::tempdir().unwrap();
    let fake = format!("ghp_{}", "A".repeat(36));
    std::fs::write(dir.path().join("output.txt"), &fake).unwrap();
    let result = ReadFile
        .execute(json!({"path": "output.txt"}), &ctx_with(dir.path(), true))
        .unwrap();
    assert_eq!(result, "[REDACTED]");
    assert!(!result.contains(&fake));
}

#[test]
fn protected_missing_edit_is_blocked_before_file_check() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(|_| true),
        Box::new(|access| match access {
            Access::Write(".nosis/new.toml") => Guard::Block("protected path".into()),
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );

    let result = EditFile
        .execute(
            json!({"path": ".nosis/new.toml", "old_string": "before", "new_string": "after"}),
            &ctx,
        )
        .unwrap();

    assert_eq!(result, "blocked by law: protected path");
}

#[test]
fn edit_ask_uses_normalized_relative_path_and_denial_is_ok_shaped() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("nested")).unwrap();
    std::fs::write(dir.path().join("note.txt"), "before").unwrap();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let approvals = Arc::clone(&seen);
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |action| {
            approvals.lock().unwrap().push(action.to_string());
            false
        }),
        Box::new(|access| match access {
            Access::Write("note.txt") => Guard::Ask,
            _ => Guard::Allow,
        }),
        empty_test_scrubber(),
    );

    let result = EditFile
        .execute(
            json!({"path": "nested/../note.txt", "old_string": "before", "new_string": "after"}),
            &ctx,
        )
        .unwrap();

    assert_eq!(
        result,
        "user denied: edit note.txt (requested as nested/../note.txt)"
    );
    assert_eq!(
        *seen.lock().unwrap(),
        ["edit note.txt (requested as nested/../note.txt)"]
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("note.txt")).unwrap(),
        "before"
    );
}

#[test]
fn explicit_permissive_guard_allows_edit_and_routes_exec_through_approval() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "before").unwrap();
    let approvals = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&approvals);
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            false
        }),
        permissive_test_guard(),
        empty_test_scrubber(),
    );

    let edited = EditFile
        .execute(
            json!({"path": "note.txt", "old_string": "before", "new_string": "after"}),
            &ctx,
        )
        .unwrap();
    let denied = ExecShell
        .execute(json!({"command": "echo should-not-run"}), &ctx)
        .unwrap();

    assert_eq!(edited, "edited note.txt");
    assert_eq!(denied, "user denied: echo should-not-run");
    assert_eq!(approvals.load(Ordering::SeqCst), 1);
}

#[test]
fn blocked_exec_never_runs_or_asks() {
    let dir = tempfile::tempdir().unwrap();
    let approvals = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&approvals);
    let ctx = ToolCtx::new(
        dir.path().to_path_buf(),
        Box::new(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            true
        }),
        Box::new(|_| Guard::Block("blocked command".into())),
        empty_test_scrubber(),
    );

    let result = ExecShell
        .execute(json!({"command": "echo pwned > marker.txt"}), &ctx)
        .unwrap();

    assert_eq!(result, "blocked by law: blocked command");
    assert_eq!(approvals.load(Ordering::SeqCst), 0);
    assert!(!dir.path().join("marker.txt").exists());
}
