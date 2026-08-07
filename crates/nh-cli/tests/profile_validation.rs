use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output};
use std::sync::mpsc;
use std::time::Duration;

const ROUTE_ID: &str = "profile-fixture";
const KEY_ENV: &str = "NH_PROFILE_FIXTURE_KEY";
const UNKNOWN_PROFILE: &str = "missing-cost-profile";

struct Fixture {
    repo: tempfile::TempDir,
    home: tempfile::TempDir,
}

impl Fixture {
    fn new(base_url: &str, user_profiles: Option<&str>, repo_profiles: Option<&str>) -> Self {
        let repo = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let home_nosis = home.path().join(".nosis");
        std::fs::create_dir_all(&home_nosis).unwrap();

        let catalog = format!(
            r#"[routes.{ROUTE_ID}]
provider = "fixture"
model_id = "profile-fixture-model"
base_url = "{base_url}"
wire = "openai"
vault_entry = "profile-fixture"
context = 128000
max_out = 64000
"#
        );
        std::fs::write(repo.path().join("catalog.toml"), &catalog).unwrap();
        std::fs::write(home_nosis.join("catalog.toml"), catalog).unwrap();
        std::fs::write(
            home_nosis.join("law.toml"),
            format!("[credential.profile-fixture]\naudience = [\"{base_url}\"]\n"),
        )
        .unwrap();

        if let Some(profiles) = user_profiles {
            std::fs::write(home_nosis.join("profiles.toml"), profiles).unwrap();
        }
        if let Some(profiles) = repo_profiles {
            let repo_nosis = repo.path().join(".nosis");
            std::fs::create_dir_all(&repo_nosis).unwrap();
            std::fs::write(repo_nosis.join("profiles.toml"), profiles).unwrap();
        }

        Self { repo, home }
    }

    fn invoke(&self, args: &[&str], with_key: bool) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nh"));
        command
            .current_dir(self.repo.path())
            .env("USERPROFILE", self.home.path())
            .env("HOME", self.home.path())
            .env_remove(KEY_ENV)
            .args(args);
        if with_key {
            command.env(KEY_ENV, "test-only-value");
        }
        command.output().unwrap()
    }

    fn receipt_path(&self) -> std::path::PathBuf {
        self.repo.path().join(".nosis").join("receipts.jsonl")
    }
}

fn assert_unknown_profile_rejected(args: &[&str]) {
    let fixture = Fixture::new("https://example.invalid", None, None);
    let output = fixture.invoke(args, false);
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(output.status.code(), Some(1), "stderr: {stderr}");
    assert!(
        stderr.contains(
            "nh: unknown profile 'missing-cost-profile'. Run `nh profile` to list choices."
        ),
        "stderr: {stderr}"
    );
    assert!(
        !stderr.contains("warning: unknown profile"),
        "stderr: {stderr}"
    );
    assert!(!fixture.receipt_path().exists());
}

#[test]
fn run_rejects_an_unknown_profile() {
    assert_unknown_profile_rejected(&[
        "run",
        "do not call the provider",
        "--model",
        ROUTE_ID,
        "--profile",
        UNKNOWN_PROFILE,
    ]);
}

#[test]
fn chat_rejects_an_unknown_profile() {
    assert_unknown_profile_rejected(&["chat", "--model", ROUTE_ID, "--profile", UNKNOWN_PROFILE]);
}

#[test]
fn tui_rejects_an_unknown_profile() {
    assert_unknown_profile_rejected(&["tui", "--model", ROUTE_ID, "--profile", UNKNOWN_PROFILE]);
}

fn read_json_request(stream: &mut TcpStream) -> serde_json::Value {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk).unwrap();
        assert!(read > 0, "client closed before sending HTTP headers");
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    while bytes.len() < header_end + content_length {
        let read = stream.read(&mut chunk).unwrap();
        assert!(read > 0, "client closed before sending the HTTP body");
        bytes.extend_from_slice(&chunk[..read]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + content_length]).unwrap()
}

fn mock_provider() -> (String, mpsc::Receiver<serde_json::Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_json_request(&mut stream);
        let body = serde_json::json!({
            "choices": [{
                "message": {"role": "assistant", "content": "ready"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 2, "completion_tokens": 1}
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        send.send(request).unwrap();
    });
    (format!("http://{address}"), receive)
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("ready"));
}

#[test]
fn run_accepts_a_known_profile() {
    let (base_url, requests) = mock_provider();
    let fixture = Fixture::new(&base_url, None, None);

    let output = fixture.invoke(
        &[
            "run",
            "reply when ready",
            "--model",
            ROUTE_ID,
            "--profile",
            "balanced",
        ],
        true,
    );

    assert_success(&output);
    let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(request["max_tokens"], serde_json::json!(16_384));
    assert!(fixture.receipt_path().exists());
}

#[test]
fn run_accepts_a_custom_profile_with_repo_tightening() {
    let (base_url, requests) = mock_provider();
    let user_profiles = r#"[profiles.careful]
thinking = "default"
max_output_tokens = 2048
"#;
    let repo_profiles = r#"[profiles.careful]
thinking = "floor"
max_output_tokens = 1024
prefer_offpeak = true
"#;
    let fixture = Fixture::new(&base_url, Some(user_profiles), Some(repo_profiles));

    let output = fixture.invoke(
        &[
            "run",
            "reply when ready",
            "--model",
            ROUTE_ID,
            "--profile",
            "careful",
        ],
        true,
    );

    assert_success(&output);
    let request = requests.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(request["max_tokens"], serde_json::json!(1_024));
    let receipt = std::fs::read_to_string(fixture.receipt_path()).unwrap();
    assert!(receipt.contains(r#""effective_profile":"careful""#));
}
