use nh_tools::{EditFile, ReadFile, Tool, ToolCtx, WriteFile};
use serde_json::json;
use std::io::Write as _;
use std::path::Path;

fn tool_ctx(workdir: &Path) -> ToolCtx {
    ToolCtx::new(workdir.to_path_buf(), Box::new(|_| true))
}

#[cfg(unix)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_file_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

#[cfg(unix)]
fn create_dir_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_dir_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

fn report_symlink_skip(test_name: &str, error: &std::io::Error) {
    // Write directly so the reason remains visible without `--nocapture`.
    let mut stderr = std::io::stderr().lock();
    let _ = writeln!(
        stderr,
        "SKIP {test_name}: symlink fixture could not be created: {error}"
    );
}

fn assert_symlink_fixture(link: &Path) {
    let metadata = std::fs::symlink_metadata(link)
        .expect("symlink fixture must exist after successful creation");
    assert!(
        metadata.file_type().is_symlink(),
        "fixture is not a symlink: {}",
        link.display()
    );
}

#[test]
fn write_file_refuses_parent_traversal_above_workdir() {
    let root = tempfile::tempdir().expect("temporary root");
    let workdir = root.path().join("workdir");
    std::fs::create_dir(&workdir).expect("workdir");

    let error = WriteFile
        .execute(
            json!({"path": "../outside.txt", "content": "must not be written"}),
            &tool_ctx(&workdir),
        )
        .expect_err("parent traversal must be refused")
        .to_string();

    assert!(
        error.contains("escapes the working directory"),
        "unexpected error: {error}"
    );
    assert!(!root.path().join("outside.txt").exists());
}

#[test]
fn write_file_refuses_absolute_path_outside_workdir() {
    let root = tempfile::tempdir().expect("temporary root");
    let workdir = root.path().join("workdir");
    std::fs::create_dir(&workdir).expect("workdir");
    let outside = root.path().join("outside.txt");

    let error = WriteFile
        .execute(
            json!({
                "path": outside.to_string_lossy().into_owned(),
                "content": "must not be written"
            }),
            &tool_ctx(&workdir),
        )
        .expect_err("absolute path outside the workdir must be refused")
        .to_string();

    assert!(
        error.contains("escapes the working directory"),
        "unexpected error: {error}"
    );
    assert!(!outside.exists());
}

#[test]
fn read_file_refuses_parent_traversal_above_workdir() {
    let root = tempfile::tempdir().expect("temporary root");
    let workdir = root.path().join("workdir");
    std::fs::create_dir(&workdir).expect("workdir");
    std::fs::write(root.path().join("outside.txt"), "outside").expect("outside read fixture");

    let error = ReadFile
        .execute(json!({"path": "../outside.txt"}), &tool_ctx(&workdir))
        .expect_err("read traversal must be refused")
        .to_string();

    assert!(
        error.contains("escapes the working directory"),
        "unexpected error: {error}"
    );
}

#[test]
fn write_file_refuses_to_replace_existing_file() {
    let workdir = tempfile::tempdir().expect("temporary workdir");
    let existing = workdir.path().join("existing.txt");
    std::fs::write(&existing, "before").expect("existing file fixture");

    let result = WriteFile
        .execute(
            json!({"path": "existing.txt", "content": "after"}),
            &tool_ctx(workdir.path()),
        )
        .expect("existing paths produce an explicit refusal");

    assert_eq!(
        result,
        "refused: existing.txt already exists - use edit_file to change it"
    );
    assert_eq!(
        std::fs::read_to_string(existing).expect("read unchanged fixture"),
        "before"
    );
}

#[test]
fn edit_file_refuses_symlink_to_file_outside_workdir() {
    let root = tempfile::tempdir().expect("temporary root");
    let workdir = root.path().join("workdir");
    std::fs::create_dir(&workdir).expect("workdir");
    let outside = root.path().join("outside.txt");
    std::fs::write(&outside, "before").expect("outside edit fixture");
    let link = workdir.join("linked.txt");

    let link_result = create_file_symlink(&outside, &link);
    if let Err(error) = link_result {
        report_symlink_skip("edit_file_refuses_symlink_to_file_outside_workdir", &error);
        return;
    }
    assert_symlink_fixture(&link);

    let error = EditFile
        .execute(
            json!({
                "path": "linked.txt",
                "old_string": "before",
                "new_string": "after"
            }),
            &tool_ctx(&workdir),
        )
        .expect_err("editing through an outside symlink must be refused")
        .to_string();

    assert!(
        error.contains("escapes the working directory"),
        "unexpected error: {error}"
    );
    assert_eq!(
        std::fs::read_to_string(outside).expect("read unchanged outside file"),
        "before"
    );
}

#[test]
fn write_file_refuses_path_through_symlinked_directory_outside_workdir() {
    let root = tempfile::tempdir().expect("temporary root");
    let workdir = root.path().join("workdir");
    let outside = root.path().join("outside");
    std::fs::create_dir(&workdir).expect("workdir");
    std::fs::create_dir(&outside).expect("outside directory fixture");
    let link = workdir.join("linked-dir");

    let link_result = create_dir_symlink(&outside, &link);
    if let Err(error) = link_result {
        report_symlink_skip(
            "write_file_refuses_path_through_symlinked_directory_outside_workdir",
            &error,
        );
        return;
    }
    assert_symlink_fixture(&link);

    let error = WriteFile
        .execute(
            json!({"path": "linked-dir/new.txt", "content": "must not be written"}),
            &tool_ctx(&workdir),
        )
        .expect_err("writing through an outside directory symlink must be refused")
        .to_string();

    assert!(
        error.contains("escapes the working directory"),
        "unexpected error: {error}"
    );
    assert!(!outside.join("new.txt").exists());
}

#[test]
fn write_file_refuses_existing_symlink_destination() {
    let root = tempfile::tempdir().expect("temporary root");
    let workdir = root.path().join("workdir");
    std::fs::create_dir(&workdir).expect("workdir");
    let outside_target = root.path().join("outside-target.txt");
    let link = workdir.join("linked.txt");

    let link_result = create_file_symlink(&outside_target, &link);
    if let Err(error) = link_result {
        report_symlink_skip("write_file_refuses_existing_symlink_destination", &error);
        return;
    }
    assert_symlink_fixture(&link);

    let result = WriteFile
        .execute(
            json!({"path": "linked.txt", "content": "must not be written"}),
            &tool_ctx(&workdir),
        )
        .expect("an existing symlink produces an explicit refusal");

    assert_eq!(
        result,
        "refused: linked.txt already exists - use edit_file to change it"
    );
    assert_symlink_fixture(&link);
    assert!(!outside_target.exists());
}
