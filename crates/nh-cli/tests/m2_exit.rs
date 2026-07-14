use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::mpsc;

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

fn reply(stream: &mut TcpStream, body: &serde_json::Value) {
    let body = body.to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
}

fn mock_provider() -> (String, mpsc::Receiver<Vec<serde_json::Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut first, _) = listener.accept().unwrap();
        let first_request = read_json_request(&mut first);
        let arguments = serde_json::json!({
            "path": ".nosis/law.toml",
            "old_string": "[write]",
            "new_string": "[write_changed]"
        })
        .to_string();
        reply(
            &mut first,
            &serde_json::json!({
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "tool_calls": [{
                            "id": "protected-edit",
                            "type": "function",
                            "function": {"name": "edit_file", "arguments": arguments}
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {
                    "prompt_tokens": 100,
                    "completion_tokens": 5,
                    "prompt_tokens_details": {"cached_tokens": 80}
                }
            }),
        );

        let (mut second, _) = listener.accept().unwrap();
        let second_request = read_json_request(&mut second);
        let tool_result = second_request["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["content"]
            .as_str()
            .unwrap()
            .to_string();
        reply(
            &mut second,
            &serde_json::json!({
                "choices": [{
                    "message": {"role": "assistant", "content": tool_result},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 120,
                    "completion_tokens": 4,
                    "prompt_tokens_details": {"cached_tokens": 100}
                }
            }),
        );
        send.send(vec![first_request, second_request]).unwrap();
    });
    (format!("http://{address}"), receive)
}

#[test]
fn protected_path_is_blocked_at_auto_end_to_end() {
    let repo = tempfile::tempdir().unwrap();
    let nosis = repo.path().join(".nosis");
    std::fs::create_dir(&nosis).unwrap();
    let protected = nosis.join("law.toml");
    std::fs::write(&protected, nh_law::STARTER_LAW_TOML).unwrap();
    let before = std::fs::read_to_string(&protected).unwrap();

    let (base_url, requests) = mock_provider();
    let catalog = format!(
        r#"[routes.m2-exit]
provider = "mock"
model_id = "m2-exit"
base_url = "{base_url}"
wire = "openai"
vault_entry = "m2-slice-c-exit"
context = 128000
"#
    );
    std::fs::write(repo.path().join("catalog.toml"), catalog).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_nh"))
        .current_dir(repo.path())
        .env("NH_M2_SLICE_C_EXIT_KEY", "test-only-value")
        .args([
            "run",
            "edit the protected project law",
            "--model",
            "m2-exit",
            "--autonomy",
            "auto",
        ])
        .output()
        .unwrap();

    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("blocked by law: protected path (.nosis/**) - held even at max autonomy"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("| cache 82%"), "stdout: {stdout}");
    assert_eq!(std::fs::read_to_string(&protected).unwrap(), before);

    let requests = requests.recv().unwrap();
    let tool_result = requests[1]["messages"].as_array().unwrap().last().unwrap();
    assert_eq!(tool_result["role"], "tool");
    assert!(tool_result["content"]
        .as_str()
        .unwrap()
        .starts_with("blocked by law:"));
}
