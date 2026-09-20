use std::process::{Command, Stdio};

fn run_with_pipes(directory: &std::path::Path, arguments: &[&str]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nh"))
        .current_dir(directory)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}

#[test]
fn explicit_and_no_argument_setup_refuse_piped_io_before_writes() {
    for arguments in [&[][..], &["setup"][..]] {
        let project = tempfile::tempdir().unwrap();
        let output = run_with_pipes(project.path(), arguments);
        let stderr = String::from_utf8(output.stderr).unwrap();

        assert_eq!(output.status.code(), Some(1), "{arguments:?}: {stderr}");
        assert!(
            stderr.contains("guided setup requires interactive stdin and stdout"),
            "{arguments:?}: {stderr}"
        );
        assert_eq!(std::fs::read_dir(project.path()).unwrap().count(), 0);
    }
}

#[test]
fn help_succeeds_without_starting_setup() {
    let project = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_nh"))
        .current_dir(project.path())
        .arg("--help")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(output.status.success());
    assert!(stdout.contains("setup"), "stdout: {stdout}");
    assert_eq!(std::fs::read_dir(project.path()).unwrap().count(), 0);
}
