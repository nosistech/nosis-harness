use super::*;
use chrono::TimeZone;
use nh_core::wire::{
    ChatClient, ChatMessage, ChatRequest, ChatResponse, FinishReason, RetryExhausted, ToolCallReq,
};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

const PEAK_CATALOG: &str = r#"
    [routes.peak-route]
    provider = "test"
    model_id = "peak-route"
    base_url = "https://example.invalid"
    wire = "openai"
    vault_entry = "test"

    [routes.peak-route.price]
    currency = "USD"
    unit = "per_million_tokens"
    cache_hit = 0.1
    cache_miss = 1.0
    output = 2.0
    price_confidence = "confirmed"

    [routes.peak-route.price.peak]
    multiplier = 2.0
    timezone = "Asia/Shanghai"
    windows = ["09:00-12:00"]
"#;

fn permissive_test_guard() -> nh_tools::GuardFn {
    Box::new(|access| match access {
        nh_tools::Access::Exec(_) => nh_tools::Guard::Ask,
        _ => nh_tools::Guard::Allow,
    })
}

fn mcp_config(name: &str, url: &str, trust: McpTrust) -> McpServerConfig {
    McpServerConfig {
        name: name.into(),
        url: url.into(),
        spec: "2026-07-28".into(),
        auth: McpAuth::None,
        scopes: Vec::new(),
        default_mode: None,
        trust,
    }
}

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

fn must_symlink_file(target: &Path, link: &Path) {
    symlink_file(target, link).unwrap_or_else(|error| {
        panic!(
            "symlink creation is required for this security test; enable Windows Developer Mode or run with symlink privilege: {error}"
        )
    });
}

#[test]
fn run_refuses_more_than_four_images_before_loading() {
    assert!(validate_image_count(4).is_ok());
    assert_eq!(
        validate_image_count(5).unwrap_err().to_string(),
        "a message can attach at most 4 images - remove the extra image paths"
    );
}

#[test]
fn finds_catalog_walking_up_from_nested_dir() {
    let tmp = tempfile::tempdir().unwrap();
    fs::write(tmp.path().join("catalog.toml"), BUNDLED_CATALOG).unwrap();
    let nested = tmp.path().join("a").join("b");
    fs::create_dir_all(&nested).unwrap();

    let (dir, text) = find_catalog_with_home(&nested, None).unwrap();
    assert_eq!(dir, tmp.path());
    assert_eq!(text, BUNDLED_CATALOG);
}

#[test]
fn missing_catalog_error_is_actionable() {
    let tmp = tempfile::tempdir().unwrap();
    let nested = tmp.path().join("deep").join("nowhere");
    fs::create_dir_all(&nested).unwrap();
    let err = find_catalog(&nested).unwrap_err();
    assert!(err.to_string().contains("nh init"), "got: {err}");
}

#[test]
fn repository_cannot_redefine_a_bundled_route_without_operator_trust() {
    let project = tempfile::tempdir().unwrap();
    let hostile = BUNDLED_CATALOG.replacen(
        "model_id = \"deepseek-flash\"",
        "model_id = \"unexpected-expensive-model\"",
        1,
    );
    assert_ne!(hostile, BUNDLED_CATALOG);
    fs::write(project.path().join("catalog.toml"), hostile).unwrap();

    let error = find_catalog_with_home(project.path(), None).unwrap_err();

    assert!(error.to_string().contains("not trusted"));
    assert!(error.to_string().contains("~/.nosis/catalog.toml"));
}

#[test]
fn bundled_deepseek_flash_accepts_images_and_pro_refuses_them() {
    let resolver = RouteResolver::from_toml(BUNDLED_CATALOG).unwrap();
    let flash = resolver.resolve("deepseek-v4-flash").unwrap();
    let pro = resolver.resolve("deepseek-v4-pro").unwrap();

    ensure_image_capable(&flash, &resolver).unwrap();
    let error = ensure_image_capable(&pro, &resolver)
        .unwrap_err()
        .to_string();
    assert!(error.contains("route deepseek-v4-pro accepts text only"));
    assert!(error.contains("deepseek-v4-flash"));
}

#[test]
fn byte_identical_user_global_catalog_is_an_explicit_trust_decision() {
    let project = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let custom = "# operator-reviewed custom catalog\n";
    fs::write(project.path().join("catalog.toml"), custom).unwrap();
    fs::create_dir_all(home.path().join(".nosis")).unwrap();
    fs::write(home.path().join(".nosis").join("catalog.toml"), custom).unwrap();

    let (root, catalog) = find_catalog_with_home(project.path(), Some(home.path())).unwrap();

    assert_eq!(root, project.path());
    assert_eq!(catalog, custom);
}

#[test]
fn symlinked_repository_catalog_is_a_hard_error() {
    let project = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("catalog.toml");
    fs::write(&target, BUNDLED_CATALOG).unwrap();
    must_symlink_file(&target, &project.path().join("catalog.toml"));

    let error = find_catalog_with_home(project.path(), None).unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains("refused repository catalog.toml"),
        "got: {message}"
    );
    assert!(message.contains("not a regular file"), "got: {message}");
}

#[test]
fn symlinked_repo_mcp_is_refused_and_user_config_continues() {
    let repo = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".nosis")).unwrap();
    fs::create_dir_all(home.path().join(".nosis")).unwrap();
    fs::write(
        home.path().join(".nosis").join("mcp.toml"),
        r#"
        [servers.shared]
        url = "https://user.example/mcp"
        trust = "auto"
        "#,
    )
    .unwrap();
    let target = outside.path().join("mcp.toml");
    fs::write(
        &target,
        r#"
        [servers.shared]
        url = "https://outside.example/mcp"
        trust = "block"
        "#,
    )
    .unwrap();
    must_symlink_file(&target, &repo.path().join(".nosis").join("mcp.toml"));
    let law = nh_law::load(repo.path(), &LoadOptions { cli_autonomy: None });
    let mut warnings = Vec::new();

    let configs =
        load_and_vet_mcp_configs(repo.path(), Some(home.path()), &law.policy, &mut warnings);

    assert_eq!(configs.len(), 1);
    assert_eq!(configs[0].name, "shared");
    assert_eq!(configs[0].trust, McpTrust::Auto);
    assert_eq!(warnings.len(), 1, "got: {warnings:?}");
    assert!(warnings[0].contains("repository .nosis/mcp.toml"));
    assert!(warnings[0].contains("not a regular file"));
    assert!(warnings[0].contains("continuing without MCP"));
}

#[test]
fn oversized_repo_mcp_is_refused_before_parsing() {
    let repo = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".nosis")).unwrap();
    fs::write(
        repo.path().join(".nosis").join("mcp.toml"),
        " ".repeat(64 * 1024 + 1),
    )
    .unwrap();
    let law = nh_law::load(repo.path(), &LoadOptions { cli_autonomy: None });
    let mut warnings = Vec::new();

    let configs =
        load_and_vet_mcp_configs(repo.path(), Some(home.path()), &law.policy, &mut warnings);

    assert!(configs.is_empty());
    assert_eq!(warnings.len(), 1, "got: {warnings:?}");
    assert!(warnings[0].contains("repository .nosis/mcp.toml"));
    assert!(warnings[0].contains("exceeds 65536 bytes"));
    assert!(warnings[0].contains("continuing without MCP"));
}

#[test]
fn mcp_audience_checks_use_exact_origins() {
    let approved = vec!["api.deepseek.com".to_string()];
    let api = McpServerConfig {
        name: "api".into(),
        url: "https://evil.example/mcp".into(),
        spec: "2026-07-28".into(),
        auth: McpAuth::ApiKey {
            vault_entry: "deepseek".into(),
        },
        scopes: Vec::new(),
        default_mode: None,
        trust: nh_tools::McpTrust::Ask,
    };
    assert_eq!(
        unapproved_mcp_target(&api, &approved),
        Some(("deepseek", "https://evil.example/mcp"))
    );

    let mut oauth = api.clone();
    oauth.url = "https://api.deepseek.com/mcp".into();
    oauth.auth = McpAuth::OAuth2 {
        token_url: "https://evil.example/token".into(),
        client_id: "client".into(),
        vault_entry: "deepseek".into(),
    };
    assert_eq!(
        unapproved_mcp_target(&oauth, &approved),
        Some(("deepseek", "https://evil.example/token"))
    );

    let mut warnings = Vec::new();
    let kept = filter_mcp_audiences_with(vec![api, oauth], &mut warnings, |_| approved.clone());
    assert!(kept.is_empty());
    assert_eq!(warnings.len(), 2, "one warning per dropped server");
    assert!(warnings.iter().all(|warning| warning.contains("dropped")));
}

#[test]
fn unparseable_destination_warning_uses_the_canonical_placeholder() {
    let mut config = mcp_config("broken", "", McpTrust::Ask);
    config.auth = McpAuth::ApiKey {
        vault_entry: "service-key".into(),
    };
    let mut warnings = Vec::new();

    let kept = filter_mcp_audiences_with(vec![config], &mut warnings, |_| Vec::new());

    assert!(kept.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].ends_with("is not approved for <unparseable destination>"));
}

#[test]
fn repo_only_mcp_server_is_dropped() {
    let repo = vec![mcp_config(
        "repo-auto",
        "https://public.example/mcp",
        McpTrust::Auto,
    )];
    let mut warnings = Vec::new();

    let merged = merge_and_vet(Vec::new(), repo, |_| Vec::new(), &mut warnings);

    assert!(merged.is_empty());
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0],
        "mcp server \"repo-auto\": repository config cannot introduce a destination - \
         declare it in ~/.nosis/mcp.toml first; dropped"
    );
}

#[test]
fn repo_only_link_local_mcp_server_is_dropped() {
    let repo = vec![mcp_config(
        "metadata",
        "http://169.254.169.254/latest",
        McpTrust::Ask,
    )];
    let mut warnings = Vec::new();

    let merged = merge_and_vet(Vec::new(), repo, |_| Vec::new(), &mut warnings);

    assert!(merged.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("cannot introduce a destination"));
    assert!(warnings[0].contains("dropped"));
}

#[test]
fn user_global_link_local_mcp_server_is_kept() {
    let user = vec![mcp_config(
        "operator-local",
        "http://169.254.169.254/mcp",
        McpTrust::Auto,
    )];
    let mut warnings = Vec::new();

    let merged = merge_and_vet(user, Vec::new(), |_| Vec::new(), &mut warnings);

    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].trust, McpTrust::Auto);
    assert!(
        warnings
            .iter()
            .all(|warning| !warning.contains("link-local/metadata")),
        "{warnings:?}"
    );
}

#[test]
fn repo_block_tightens_shared_user_global_auto_server() {
    let user = vec![mcp_config(
        "shared",
        "https://user.example/mcp",
        McpTrust::Auto,
    )];
    let repo = vec![mcp_config(
        "shared",
        "https://repo.example/mcp",
        McpTrust::Block,
    )];
    let mut warnings = Vec::new();

    let merged = merge_and_vet(user, repo, |_| Vec::new(), &mut warnings);

    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].trust, McpTrust::Block);
    assert!(warnings.is_empty());
}

#[test]
fn repo_auto_cannot_loosen_shared_user_global_ask_server() {
    let user = vec![mcp_config(
        "shared",
        "https://user.example/mcp",
        McpTrust::Ask,
    )];
    let repo = vec![mcp_config(
        "shared",
        "https://repo.example/mcp",
        McpTrust::Auto,
    )];
    let mut warnings = Vec::new();

    let merged = merge_and_vet(user, repo, |_| Vec::new(), &mut warnings);

    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].trust, McpTrust::Ask);
    assert!(warnings.is_empty());
}

#[test]
fn shared_server_destination_and_auth_come_from_user_global_config() {
    let mut user = mcp_config("shared", "https://user.example/mcp", McpTrust::Auto);
    let user_auth = McpAuth::ApiKey {
        vault_entry: "user-entry".into(),
    };
    user.auth = user_auth.clone();
    let mut repo = mcp_config("shared", "http://169.254.169.254/mcp", McpTrust::Ask);
    repo.auth = McpAuth::ApiKey {
        vault_entry: "repo-entry".into(),
    };
    let mut warnings = Vec::new();

    let merged = merge_and_vet(
        vec![user],
        vec![repo],
        |entry| {
            assert_eq!(entry, "user-entry");
            vec!["user.example".into()]
        },
        &mut warnings,
    );

    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].url, "https://user.example/mcp");
    assert_eq!(merged[0].auth, user_auth);
    assert_eq!(merged[0].trust, McpTrust::Ask);
    assert!(warnings.is_empty());
}

#[test]
fn merged_mcp_server_with_unapproved_credential_is_dropped() {
    let mut user = mcp_config("credentialed", "https://api.example/mcp", McpTrust::Ask);
    user.auth = McpAuth::ApiKey {
        vault_entry: "operator-secret".into(),
    };
    let mut warnings = Vec::new();

    let merged = merge_and_vet(
        vec![user],
        Vec::new(),
        |_| vec!["other.example".into()],
        &mut warnings,
    );

    assert!(merged.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("credential \"operator-secret\""));
    assert!(warnings[0].contains("dropped"));
}

#[test]
fn approval_codes_are_fixed_width_uppercase_hex() {
    assert_eq!(encode_approval_code([0x00, 0xab, 0x10, 0xff]), "00AB10FF");
}

#[test]
fn approval_code_match_removes_only_one_terminal_line_ending() {
    let code = "A1B2C3D4";
    assert!(approval_line_matches("A1B2C3D4\n", code));
    assert!(approval_line_matches("A1B2C3D4\r\n", code));
    for line in [
        "A1B2C3D4",
        "A1B2C3D4\n\n",
        "A1B2C3D4\r\n\r\n",
        " A1B2C3D4\n",
        "A1B2C3D4 \n",
        "a1b2c3d4\n",
        "yes\n",
        "\n",
    ] {
        assert!(!approval_line_matches(line, code), "accepted {line:?}");
    }
}

#[test]
fn think_flag_resolves_within_route_capability() {
    let cases = [
        (ThinkArg::None, ThinkingEffort::None),
        (ThinkArg::Low, ThinkingEffort::Low),
        (ThinkArg::High, ThinkingEffort::High),
        (ThinkArg::Max, ThinkingEffort::Max),
    ];
    for (arg, want) in cases {
        assert_eq!(
            effort_for(
                Some(arg),
                ThinkingPosture::Default,
                ThinkingDialect::DeepseekNhm,
                Wire::OpenAi,
            ),
            want
        );
        assert_eq!(
            effort_for(
                Some(arg),
                ThinkingPosture::Default,
                ThinkingDialect::AlwaysThinking,
                Wire::OpenAi,
            ),
            ThinkingEffort::High
        );
    }
}

#[test]
fn anthropic_wire_effort_is_provider_default() {
    assert_eq!(
        effort_for(
            Some(ThinkArg::High),
            ThinkingPosture::Ceiling,
            ThinkingDialect::DeepseekNhm,
            Wire::AnthropicMessages,
        ),
        ThinkingEffort::None
    );
}

#[test]
fn think_default_follows_route_dialect() {
    // Always-thinking and high/max-only routes run at High; effort-toggle
    // routes stay at None until the user asks (cheap by default).
    assert_eq!(
        effort_for(
            None,
            ThinkingPosture::Default,
            ThinkingDialect::AlwaysThinking,
            Wire::OpenAi,
        ),
        ThinkingEffort::High
    );
    assert_eq!(
        effort_for(
            None,
            ThinkingPosture::Default,
            ThinkingDialect::GlmHm,
            Wire::OpenAi,
        ),
        ThinkingEffort::High
    );
    assert_eq!(
        effort_for(
            None,
            ThinkingPosture::Default,
            ThinkingDialect::DeepseekNhm,
            Wire::OpenAi,
        ),
        ThinkingEffort::None
    );
    assert_eq!(
        effort_for(
            None,
            ThinkingPosture::Default,
            ThinkingDialect::None,
            Wire::OpenAi,
        ),
        ThinkingEffort::None
    );
}

#[test]
fn autonomy_mapping_is_optional_and_exact() {
    assert_eq!(autonomy_for(None), None);
    assert_eq!(autonomy_for(Some(AutonomyArg::Ask)), Some(Autonomy::Ask));
    assert_eq!(autonomy_for(Some(AutonomyArg::Auto)), Some(Autonomy::Auto));
}

#[test]
fn safe_line_redacts_key_shapes_and_literals_before_stderr() {
    let scrubber = Scrubber::new(vec!["fake-literal-secret".to_string()]);
    let line = safe_line(
        &scrubber,
        "curl -H 'Authorization: sk-test-00000000' fake-literal-secret\x1b[1A",
    );
    assert!(!line.contains("sk-test-00000000"), "got: {line}");
    assert!(!line.contains("fake-literal-secret"), "got: {line}");
    assert!(line.contains("[REDACTED]"), "got: {line}");
    assert!(!line.chars().any(|c| c.is_control()), "got: {line}");
}

#[test]
fn run_approval_display_preserves_long_command_without_truncating() {
    let action = format!("exec {}", "x".repeat(700));
    let display = approval_line(&Scrubber::new(Vec::new()), &action);

    assert_eq!(display, action);
    assert!(!display.contains("more chars"));
}

#[test]
fn safe_text_neutralizes_terminal_controls_and_preserves_newlines() {
    let scrubber = Scrubber::new(Vec::new());
    let answer = "ordinary\x1b]0;pwned\x07\nnext\x1b[31m\n";
    let safe = safe_text(&scrubber, answer);

    assert!(!safe.contains('\x1b'), "got: {safe}");
    assert!(!safe.contains('\x07'), "got: {safe}");
    assert!(safe.starts_with("ordinary"), "got: {safe}");
    assert!(safe.contains("\nnext"), "got: {safe}");
    assert!(safe.ends_with('\n'), "got: {safe}");
}

#[test]
fn safe_text_preserves_long_answer_lines() {
    let answer = "x".repeat(2_000);
    let safe = safe_text(&Scrubber::new(Vec::new()), &answer);

    assert_eq!(safe, answer);
    assert!(!safe.contains("more chars"));
}

#[test]
fn missing_usage_is_reported_without_a_zero_cost() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
    let lines = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(None, None),
        &Default::default(),
        2,
        1,
        RunTiming {
            started: at,
            ended: at,
        },
    );

    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("tokens: not reported by provider - cost unknown"));
    assert!(!lines[0].contains("cost $0.00"));
}

#[test]
fn run_meter_context_segment_obeys_window_and_usage_evidence() {
    let catalog = PEAK_CATALOG.replacen(
        "vault_entry = \"test\"",
        "vault_entry = \"test\"\n    context = 1000",
        1,
    );
    let resolver = RouteResolver::from_toml(&catalog).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
    let timing = RunTiming {
        started: at,
        ended: at,
    };
    let measured = Usage {
        prompt_tokens: 250,
        completion_tokens: 20,
        cached_tokens: None,
        evidence: UsageEvidence::Measured,
    };
    let render = |route: &nh_routes::ResolvedRoute, usage: Option<&Usage>| {
        run_meter_lines(
            &resolver,
            route,
            RunUsage::new(usage, usage),
            &Default::default(),
            1,
            0,
            timing,
        )[0]
        .clone()
    };

    let line = render(&route, Some(&measured));
    assert!(
        line.contains("tokens 250 in / 20 out | ctx 25%"),
        "got: {line}"
    );

    let partial = Usage {
        evidence: UsageEvidence::Partial,
        ..measured.clone()
    };
    let line = render(&route, Some(&partial));
    assert!(line.contains("(lower bound) | ctx ~25%"), "got: {line}");

    let unknown = Usage {
        evidence: UsageEvidence::Unknown,
        ..measured.clone()
    };
    let line = render(&route, Some(&unknown));
    assert!(!line.contains("ctx"), "got: {line}");
    assert!(!line.contains("ctx 0"), "got: {line}");
    let line = render(&route, None);
    assert!(!line.contains("ctx"), "got: {line}");
    assert!(!line.contains("ctx 0"), "got: {line}");

    let over_window = Usage {
        prompt_tokens: 1_250,
        ..measured.clone()
    };
    let line = render(&route, Some(&over_window));
    assert!(line.contains("| ctx 125%"), "got: {line}");
    assert!(!line.contains("| ctx 100%"), "got: {line}");

    let zero_catalog = catalog.replace("context = 1000", "context = 0");
    let zero_resolver = RouteResolver::from_toml(&zero_catalog).unwrap();
    let zero_route = zero_resolver.resolve("peak-route").unwrap();
    let lines = run_meter_lines(
        &zero_resolver,
        &zero_route,
        RunUsage::new(Some(&measured), Some(&measured)),
        &Default::default(),
        1,
        0,
        timing,
    );
    assert!(lines[0].contains("| ctx inf%"), "got: {}", lines[0]);

    let resolver_without_context = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route_without_context = resolver_without_context.resolve("peak-route").unwrap();
    let lines = run_meter_lines(
        &resolver_without_context,
        &route_without_context,
        RunUsage::new(Some(&measured), Some(&measured)),
        &Default::default(),
        1,
        0,
        timing,
    );
    assert!(!lines[0].contains("ctx"), "got: {}", lines[0]);
}

#[test]
fn run_meter_marks_tiny_measured_context_occupancy() {
    let catalog = PEAK_CATALOG.replacen(
        "vault_entry = \"test\"",
        "vault_entry = \"test\"\n    context = 1000000",
        1,
    );
    let resolver = RouteResolver::from_toml(&catalog).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let usage = Usage {
        prompt_tokens: 4_000,
        completion_tokens: 20,
        cached_tokens: None,
        evidence: UsageEvidence::Measured,
    };
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
    let lines = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(Some(&usage), Some(&usage)),
        &Default::default(),
        1,
        0,
        RunTiming {
            started: at,
            ended: at,
        },
    );

    assert!(lines[0].contains("| ctx <1%"), "got: {}", lines[0]);
    assert!(!lines[0].contains("| ctx 0%"), "got: {}", lines[0]);
}

struct MeasuredThenUnmeteredSuccess {
    calls: AtomicUsize,
}

impl ChatClient for MeasuredThenUnmeteredSuccess {
    fn complete(&self, _request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Ok(ChatResponse {
                message: serde_json::from_value(serde_json::json!({
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call-1",
                        "name": "read_file",
                        "arguments": "{\"path\":\"evidence.txt\"}"
                    }]
                }))
                .unwrap(),
                finish_reason: "tool_calls".into(),
                usage: Some(Usage {
                    prompt_tokens: 100,
                    completion_tokens: 20,
                    cached_tokens: Some(10),
                    evidence: UsageEvidence::Measured,
                }),
                retries: Default::default(),
            });
        }
        Ok(ChatResponse {
            message: serde_json::from_value(serde_json::json!({
                "role": "assistant",
                "content": "done"
            }))
            .unwrap(),
            finish_reason: "stop".into(),
            usage: None,
            retries: Default::default(),
        })
    }
}

#[test]
fn real_measured_then_unmetered_run_is_a_marked_lower_bound_and_refuses_cost() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("evidence.txt"), b"real tool input").unwrap();
    let catalog = PEAK_CATALOG.replacen(
        "vault_entry = \"test\"",
        "vault_entry = \"test\"\n    context = 1000",
        1,
    );
    let resolver = RouteResolver::from_toml(&catalog).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
    let last_request_usage = LastRequestUsage::default();
    let mut agent = AgentLoop {
        client: last_request_usage.wrap(Box::new(MeasuredThenUnmeteredSuccess {
            calls: AtomicUsize::new(0),
        })),
        tools: builtin_tools(),
        ctx: ToolCtx::new(
            tmp.path().to_path_buf(),
            Box::new(|_| false),
            permissive_test_guard(),
            Scrubber::new(Vec::new()),
        ),
        receipts: ReceiptWriter::for_path(
            tmp.path(),
            tmp.path().join("receipts.jsonl"),
            Scrubber::new(Vec::new()),
        ),
        model_id: route.model_id().to_owned(),
        max_turns: 2,
        thinking: ThinkingEffort::None,
        profile: None,
        constitution: None,
        context_limit: route.context(),
        on_event: None,
    };
    let (_, receipt) = agent.run("read the evidence").unwrap();
    let usage = receipt.usage.as_ref().unwrap();

    assert_eq!(usage.evidence, UsageEvidence::Partial);
    assert_eq!(receipt.turns, 2);
    assert_eq!(receipt.tool_calls, 1);
    let context_usage = last_request_usage.snapshot();
    assert_eq!(
        context_usage, None,
        "the final provider response was unmetered"
    );

    let lines = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(Some(usage), context_usage.as_ref()),
        &receipt.compaction,
        receipt.turns,
        receipt.tool_calls,
        RunTiming {
            started: at,
            ended: at,
        },
    );

    assert_eq!(
        lines,
        vec![
            "turns 2 | tool calls 1 | tokens ~100 in / ~20 out / ~10 cached (lower bound)",
            "cost unknown - usage is a lower bound",
        ]
    );
    assert!(!lines.iter().any(|line| line.contains("saved")));
    assert!(!lines.iter().any(|line| line.contains('$')));
}

#[test]
fn legacy_unknown_run_usage_renders_like_absence_without_leaking_counters() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
    let usage = Usage {
        prompt_tokens: 91,
        completion_tokens: 7,
        cached_tokens: Some(40),
        evidence: UsageEvidence::Unknown,
    };
    let timing = RunTiming {
        started: at,
        ended: at,
    };

    let unknown = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(Some(&usage), None),
        &Default::default(),
        1,
        0,
        timing,
    );
    let absent = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(None, None),
        &Default::default(),
        1,
        0,
        timing,
    );

    assert_eq!(unknown, absent);
    assert!(unknown[0].contains("tokens: not reported by provider - cost unknown"));
    assert!(!unknown[0].contains("91"));
    assert!(!unknown[0].contains("$0.00"));
}

#[test]
fn measured_tokens_without_cache_evidence_render_an_upper_bound() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
    let usage = Usage {
        prompt_tokens: 100,
        completion_tokens: 20,
        cached_tokens: None,
        evidence: UsageEvidence::Measured,
    };

    let lines = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(Some(&usage), None),
        &Default::default(),
        1,
        0,
        RunTiming {
            started: at,
            ended: at,
        },
    );

    assert_eq!(
        lines[1],
        "cost at most $0.0001 - cache split not reported by provider"
    );
    assert!(!lines[1].contains("cost unknown"));
    assert!(!lines.iter().any(|line| line.contains("saved")));
    assert!(!lines.iter().any(|line| line.contains("peak")));

    let verify_live_catalog = PEAK_CATALOG.replace(
        "price_confidence = \"confirmed\"",
        "price_confidence = \"verify_live\"",
    );
    let resolver = RouteResolver::from_toml(&verify_live_catalog).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let verify_live = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(Some(&usage), None),
        &Default::default(),
        1,
        0,
        RunTiming {
            started: at,
            ended: at,
        },
    );
    assert_eq!(
        verify_live[1],
        "cost at most $0.0001* - cache split not reported by provider · *price verify_live"
    );

    let cached_heavy_catalog = PEAK_CATALOG.replace("cache_hit = 0.1", "cache_hit = 3.0");
    let resolver = RouteResolver::from_toml(&cached_heavy_catalog).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let cached_heavy = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(Some(&usage), None),
        &Default::default(),
        1,
        0,
        RunTiming {
            started: at,
            ended: at,
        },
    );
    assert_eq!(
        cached_heavy[1],
        "cost at most $0.0003 - cache split not reported by provider"
    );
}

struct MeteredRunFailure;

impl ChatClient for MeteredRunFailure {
    fn complete(&self, _request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        Err(anyhow::Error::new(RetryExhausted {
            stats: Default::default(),
            usage: Some(Usage {
                prompt_tokens: 100,
                completion_tokens: 20,
                cached_tokens: Some(10),
                evidence: UsageEvidence::Measured,
            }),
            last_failure: "provider failed after metering".into(),
            attempts: 1,
            elapsed: Duration::from_millis(5),
        }))
    }
}

#[test]
fn failed_run_projects_the_real_agent_error_receipt_meter() {
    let tmp = tempfile::tempdir().unwrap();
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let mut agent = AgentLoop {
        client: Box::new(MeteredRunFailure),
        tools: Vec::new(),
        ctx: ToolCtx::new(
            tmp.path().to_path_buf(),
            Box::new(|_| false),
            permissive_test_guard(),
            Scrubber::new(Vec::new()),
        ),
        receipts: ReceiptWriter::for_path(
            tmp.path(),
            tmp.path().join("receipts.jsonl"),
            Scrubber::new(Vec::new()),
        ),
        model_id: route.model_id().to_owned(),
        max_turns: 1,
        thinking: ThinkingEffort::None,
        profile: None,
        constitution: None,
        context_limit: route.context(),
        on_event: None,
    };
    let error = agent.run("fail after metering").unwrap_err();
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();

    let lines = failed_run_meter_lines(
        &error,
        &resolver,
        &route,
        None,
        RunTiming {
            started: at,
            ended: at,
        },
    )
    .expect("the production run error path recognizes AgentRunError");

    assert_eq!(
        lines[0],
        "turns 1 | tool calls 0 | tokens 100 in / 20 out / 10 cached | cache 10%"
    );
    assert!(lines[1].starts_with("cost $0.0001"), "got: {lines:?}");
    let durable: nh_core::receipt::Receipt =
        serde_json::from_str(&std::fs::read_to_string(tmp.path().join("receipts.jsonl")).unwrap())
            .unwrap();
    assert_eq!(
        durable.usage.as_ref().map(|usage| usage.evidence),
        Some(UsageEvidence::Measured)
    );
    assert_eq!(durable.turns, 1);
}

#[test]
fn local_run_meter_uses_the_ratified_qualifier_with_or_without_usage() {
    let resolver = RouteResolver::from_toml(
        r#"
        [routes.local-test]
        provider = "ollama"
        model_id = "user-filled-model"
        base_url = "http://127.0.0.1:11434/v1"
        wire = "openai"
        vault_entry = "ollama-local"
        class = "local"
        context = 8192
        max_out = 2048
        [routes.local-test.price]
        currency = "USD"
        unit = "per_million_tokens"
        cache_hit = 0.0
        cache_miss = 0.0
        output = 0.0
        price_confidence = "confirmed"
        "#,
    )
    .unwrap();
    let route = resolver.resolve("local-test").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 7, 29, 0, 0, 0).unwrap();
    let usage = Usage {
        prompt_tokens: 100,
        completion_tokens: 20,
        cached_tokens: None,
        evidence: UsageEvidence::Measured,
    };

    let reported = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(Some(&usage), None),
        &Default::default(),
        1,
        0,
        RunTiming {
            started: at,
            ended: at,
        },
    );
    assert_eq!(
        reported,
        vec![
            "turns 1 | tool calls 0 | tokens 100 in / 20 out".to_owned(),
            nh_routes::LOCAL_METER_COPY.to_owned(),
        ]
    );
    assert!(!reported.iter().any(|line| line.contains("$0.00")));

    let missing = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(None, None),
        &Default::default(),
        1,
        0,
        RunTiming {
            started: at,
            ended: at,
        },
    );
    assert_eq!(
        missing,
        vec![
            "turns 1 | tool calls 0 | tokens: not reported by provider - cost unknown".to_owned(),
            nh_routes::LOCAL_METER_COPY.to_owned(),
        ]
    );

    let mut compaction = nh_core::receipt::CompactionStats::default();
    compaction.record(2, 50, Some(100));
    let compaction = compaction_meter_line(&resolver, &route, &compaction).unwrap();
    assert!(
        compaction.contains("~50 tokens elided"),
        "got: {compaction}"
    );
    assert!(
        compaction.contains(nh_routes::LOCAL_METER_COPY),
        "got: {compaction}"
    );
    assert!(!compaction.contains("$0.00"), "got: {compaction}");
}

#[test]
fn run_meter_distinguishes_absent_cache_measurement_from_measured_zero() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
    let absent = Usage {
        prompt_tokens: 100,
        completion_tokens: 20,
        cached_tokens: None,
        evidence: UsageEvidence::Measured,
    };
    let measured_zero = Usage {
        cached_tokens: Some(0),
        ..absent.clone()
    };

    let absent = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(Some(&absent), None),
        &Default::default(),
        1,
        0,
        RunTiming {
            started: at,
            ended: at,
        },
    );
    assert_eq!(absent[0], "turns 1 | tool calls 0 | tokens 100 in / 20 out");
    assert_eq!(
        absent[1],
        "cost at most $0.0001 - cache split not reported by provider"
    );

    let measured_zero = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(Some(&measured_zero), None),
        &Default::default(),
        1,
        0,
        RunTiming {
            started: at,
            ended: at,
        },
    );
    assert_eq!(
        measured_zero[0],
        "turns 1 | tool calls 0 | tokens 100 in / 20 out / 0 cached | cache 0%"
    );
    assert!(measured_zero[1].starts_with("cost $0.0001"));
    assert!(!measured_zero[1].contains("at most"));
}

#[test]
fn run_counterfactual_without_peak_table_drops_the_segment_cleanly() {
    let resolver = RouteResolver::from_toml(BUNDLED_CATALOG).unwrap();
    let route = resolver.resolve("kimi-k2.6").unwrap();
    assert!(route.price().unwrap().peak.is_none());
    let usage = Usage {
        prompt_tokens: 100_000,
        completion_tokens: 50_000,
        cached_tokens: Some(90_000),
        evidence: UsageEvidence::Measured,
    };
    let at = Utc.with_ymd_and_hms(2026, 8, 5, 12, 0, 0).unwrap();

    let line = turn_cost_line(&resolver, &route, &usage, at).unwrap();

    assert!(line.contains("   (no-cache "), "got: {line}");
    assert!(line.contains(" · top-tier "), "got: {line}");
    assert!(!line.contains("   (peak "), "got: {line}");
    assert!(!line.contains("( ·"), "got: {line}");
    assert!(!line.contains("· ·"), "got: {line}");
    assert!(!line.ends_with(" · )"), "got: {line}");
}

#[test]
fn ordinary_progress_and_empty_compaction_keep_the_old_surface_bytes() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();

    assert_eq!(
        meter::progress_meter_line(&resolver, &route, "calling read_file"),
        "calling read_file"
    );
    assert_eq!(
        compaction_meter_line(&resolver, &route, &Default::default()),
        None
    );
}

#[test]
fn compaction_without_preceding_cache_refuses_money_even_when_current_usage_has_cache() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
    let usage = Usage {
        prompt_tokens: 500,
        completion_tokens: 20,
        cached_tokens: Some(400),
        evidence: UsageEvidence::Measured,
    };
    let mut compaction = nh_core::receipt::CompactionStats::default();
    compaction.record(8, 100, None);

    let lines = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(Some(&usage), None),
        &compaction,
        1,
        0,
        RunTiming {
            started: at,
            ended: at,
        },
    );
    let line = lines.last().expect("compaction line");
    assert_eq!(
        line,
        "compaction 1 event · 8 messages elided · ~100 tokens elided · next-call money not stated - exact preceding-call cached tokens unavailable"
    );
    assert!(!line.contains('$'));
    assert!(!line.contains("saving"));
    assert_eq!(usage.cached_tokens, Some(400));
}

#[test]
fn compaction_prices_cache_hit_and_calls_a_negative_net_a_loss() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
    let mut compaction = nh_core::receipt::CompactionStats::default();
    compaction.record_at(8, 1_000, Some(3_000), at.timestamp());

    let line = compaction_meter_line(&resolver, &route, &compaction).unwrap();

    assert!(
        line.contains("next-call estimate: cache-hit saving ~$0.0001"),
        "got: {line}"
    );
    assert!(
        line.contains("cache-reset surcharge ~$0.0018"),
        "got: {line}"
    );
    assert!(line.contains("net loss ~$0.0017"), "got: {line}");
    assert!(!line.contains("cache-hit saving ~$0.0010"), "got: {line}");
}

#[test]
fn receipt_compaction_uses_stored_event_time_instead_of_run_end_time() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let event_at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 30, 0).unwrap();
    let ended_in_peak = Utc.with_ymd_and_hms(2026, 7, 15, 1, 30, 0).unwrap();
    let usage = Usage {
        prompt_tokens: 500,
        completion_tokens: 20,
        cached_tokens: Some(400),
        evidence: UsageEvidence::Measured,
    };
    let mut compaction = nh_core::receipt::CompactionStats::default();
    compaction.record_at(8, 1_000, Some(3_000), event_at.timestamp());

    let lines = run_meter_lines(
        &resolver,
        &route,
        RunUsage::new(Some(&usage), None),
        &compaction,
        1,
        0,
        RunTiming {
            started: event_at,
            ended: ended_in_peak,
        },
    );
    let line = lines.last().expect("compaction line");

    assert!(line.contains("net loss ~$0.0017"), "got: {line}");
    assert!(!line.contains("net loss ~$0.0034"), "got: {line}");
}

#[test]
fn out_of_range_compaction_time_refuses_money_instead_of_guessing() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let mut compaction = nh_core::receipt::CompactionStats::default();
    compaction.record_at(8, 1_000, Some(3_000), i64::MAX);

    let line = compaction_meter_line(&resolver, &route, &compaction).unwrap();

    assert!(
        line.ends_with("next-call money not stated - exact compaction time unavailable"),
        "got: {line}"
    );
}

#[test]
fn compaction_money_uses_the_existing_verify_live_asterisk_convention() {
    let catalog = PEAK_CATALOG.replace(
        "price_confidence = \"confirmed\"",
        "price_confidence = \"verify_live\"",
    );
    let resolver = RouteResolver::from_toml(&catalog).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
    let mut compaction = nh_core::receipt::CompactionStats::default();
    compaction.record_at(8, 1_000, Some(3_000), at.timestamp());

    let line = compaction_meter_line(&resolver, &route, &compaction).unwrap();

    assert!(line.contains("net loss ~$0.0017*"), "got: {line}");
    assert!(line.ends_with(" · *price verify_live"), "got: {line}");
}

#[test]
fn terminal_run_meter_preserves_unicode_or_uses_fallback_separators() {
    let catalog = PEAK_CATALOG.replace(
        "price_confidence = \"confirmed\"",
        "price_confidence = \"verify_live\"",
    );
    let resolver = RouteResolver::from_toml(&catalog).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
    let mut compaction = nh_core::receipt::CompactionStats::default();
    compaction.record_at(8, 1_000, Some(3_000), at.timestamp());
    let source = vec![compaction_meter_line(&resolver, &route, &compaction).unwrap()];

    let unicode = super::meter::terminal_meter_lines(TerminalCapability::Unicode, source.clone());
    let ascii =
        super::meter::terminal_meter_lines(TerminalCapability::AsciiFallback, source.clone());

    assert_eq!(unicode, source);
    assert!(
        unicode[0].ends_with("\u{b7} *price verify_live"),
        "got: {}",
        unicode[0]
    );
    assert!(
        ascii[0].ends_with("- *price verify_live"),
        "got: {}",
        ascii[0]
    );
}

#[test]
fn multiple_compactions_report_facts_but_refuse_one_aggregate_next_call_price() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let mut compaction = nh_core::receipt::CompactionStats::default();
    compaction.record(3, 100, Some(300));
    compaction.record(4, 200, Some(600));

    let line = compaction_meter_line(&resolver, &route, &compaction).unwrap();

    assert_eq!(
        line,
        "compaction 2 events · 7 messages elided · ~300 tokens elided · aggregate money not stated - compactions affect separate next calls"
    );
    assert!(!line.contains('$'));
}

#[test]
fn unpriced_compaction_keeps_facts_and_states_that_money_is_unavailable() {
    let resolver = RouteResolver::from_toml(
        r#"
        [routes.unpriced]
        provider = "test"
        model_id = "unpriced"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
        "#,
    )
    .unwrap();
    let route = resolver.resolve("unpriced").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
    let mut compaction = nh_core::receipt::CompactionStats::default();
    compaction.record_at(2, 100, Some(300), at.timestamp());

    let line = compaction_meter_line(&resolver, &route, &compaction).unwrap();

    assert_eq!(
        line,
        "compaction 1 event · 2 messages elided · ~100 tokens elided · next-call money not stated - no price data"
    );
}

#[test]
fn live_compaction_parses_the_real_core_event_shape() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let at = Utc.with_ymd_and_hms(2026, 7, 15, 0, 0, 0).unwrap();
    let event = nh_core::agent::CompactionEvent::new_at(72, 8, 1_000, Some(3_000), at.timestamp());

    let line = meter::progress_meter_line(&resolver, &route, &event.to_string());

    assert!(
        line.starts_with("context ~72% - compacted 8 earlier messages · ~1000 tokens elided"),
        "got: {line}"
    );
    assert!(line.contains("net loss"), "got: {line}");
}

#[test]
fn run_cost_marks_only_a_peak_boundary_crossing() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let usage = Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        cached_tokens: Some(20),
        evidence: UsageEvidence::Measured,
    };
    let before_peak = Utc.with_ymd_and_hms(2026, 7, 15, 0, 30, 0).unwrap();
    let in_peak = Utc.with_ymd_and_hms(2026, 7, 15, 1, 30, 0).unwrap();
    let later_in_peak = Utc.with_ymd_and_hms(2026, 7, 15, 2, 30, 0).unwrap();

    let crossed = turn_cost_line_for_run(&resolver, &route, &usage, before_peak, in_peak).unwrap();
    assert!(crossed.contains("*priced at run end - spans a peak boundary"));

    let steady = turn_cost_line_for_run(&resolver, &route, &usage, in_peak, later_in_peak).unwrap();
    assert!(!steady.contains("spans a peak boundary"));
}

#[test]
fn non_terminal_stdin_cannot_approve_even_with_the_current_code() {
    let mut input = std::io::Cursor::new(b"A1B2C3D4\n".to_vec());
    let mut stderr = Vec::new();

    assert!(!approve_with_io(
        "echo safe",
        false,
        &mut input,
        &mut stderr,
        || -> Option<[u8; APPROVAL_CODE_BYTES]> {
            panic!("non-terminal approval must not generate a code")
        }
    ));
    assert_eq!(input.position(), 0, "piped input must not be consumed");
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(stderr.contains("stdin is not a terminal"), "got: {stderr}");
    assert!(stderr.contains("cannot approve"), "got: {stderr}");
}

#[test]
fn terminal_stdin_requires_the_exact_current_code() {
    let mut input = std::io::Cursor::new(b"A1B2C3D4\n".to_vec());
    let mut stderr = Vec::new();

    assert!(approve_with_io(
        "echo safe",
        true,
        &mut input,
        &mut stderr,
        || Some([0xa1, 0xb2, 0xc3, 0xd4])
    ));
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        "  approve? echo safe\n  To approve this request, type A1B2C3D4 and press Enter; anything else declines [default: no]: "
    );
}

struct FailingApprovalWriter {
    fail_write: bool,
}

impl std::io::Write for FailingApprovalWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.fail_write {
            Err(std::io::Error::other("fixture write failure"))
        } else {
            Ok(buf.len())
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.fail_write {
            Ok(())
        } else {
            Err(std::io::Error::other("fixture flush failure"))
        }
    }
}

#[test]
fn prompt_io_failure_refuses_before_consuming_matching_input() {
    for fail_write in [true, false] {
        let mut input = std::io::Cursor::new(b"A1B2C3D4\n".to_vec());
        let mut stderr = FailingApprovalWriter { fail_write };

        assert!(!approve_with_io(
            "echo safe",
            true,
            &mut input,
            &mut stderr,
            || Some([0xa1, 0xb2, 0xc3, 0xd4])
        ));
        assert_eq!(input.position(), 0, "prompt failure consumed input");
    }
}

#[test]
fn stale_code_from_a_prior_prompt_cannot_approve_a_new_request() {
    let mut first_input = std::io::Cursor::new(b"A1B2C3D4\n".to_vec());
    let mut first_stderr = Vec::new();
    assert!(approve_with_io(
        "first action",
        true,
        &mut first_input,
        &mut first_stderr,
        || Some([0xa1, 0xb2, 0xc3, 0xd4])
    ));

    let mut second_input = std::io::Cursor::new(b"A1B2C3D4\n".to_vec());
    let mut second_stderr = Vec::new();
    assert!(!approve_with_io(
        "second action",
        true,
        &mut second_input,
        &mut second_stderr,
        || Some([0x12, 0x34, 0x56, 0x78])
    ));
    let second_stderr = String::from_utf8(second_stderr).unwrap();
    assert!(
        second_stderr.contains("second action"),
        "got: {second_stderr}"
    );
    assert!(second_stderr.contains("12345678"), "got: {second_stderr}");
    assert!(!second_stderr.contains("A1B2C3D4"), "got: {second_stderr}");
}

#[test]
fn ordinary_pretyped_approval_lines_default_to_deny() {
    for line in ["y\n", "yes\n", "\n", "A1B2C3D4 \n", "a1b2c3d4\n"] {
        let mut input = std::io::Cursor::new(line.as_bytes());
        let mut stderr = Vec::new();
        assert!(
            !approve_with_io("echo safe", true, &mut input, &mut stderr, || Some([
                0xa1, 0xb2, 0xc3, 0xd4
            ])),
            "accepted {line:?}"
        );
    }
}

#[test]
fn approval_fails_closed_when_fresh_randomness_is_unavailable() {
    let mut input = std::io::Cursor::new(b"A1B2C3D4\n".to_vec());
    let mut stderr = Vec::new();

    assert!(!approve_with_io(
        "echo safe",
        true,
        &mut input,
        &mut stderr,
        || None
    ));
    assert_eq!(input.position(), 0, "refusal must not consume input");
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        "  approval refused: could not generate a fresh confirmation code\n"
    );
}

#[test]
fn command_too_large_to_display_is_refused() {
    let display = "x".repeat(MAX_APPROVAL_DISPLAY_BYTES + 1);
    let mut input = std::io::Cursor::new(b"yes\n".to_vec());
    let mut stderr = Vec::new();

    assert!(!approve_with_io(
        &display,
        true,
        &mut input,
        &mut stderr,
        || -> Option<[u8; APPROVAL_CODE_BYTES]> {
            panic!("oversized approval must not generate a code")
        }
    ));
    assert_eq!(
        input.position(),
        0,
        "refusal must not consume approval input"
    );
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        format!(
            "  approval refused: command is too large to display in full (maximum {MAX_APPROVAL_DISPLAY_BYTES} bytes)\n"
        )
    );
}

#[test]
fn run_stdout_contains_only_the_answer_and_metering_uses_stderr() {
    let scrubber = Scrubber::new(Vec::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let meter = vec!["tokens 12 in / 7 out".to_owned(), "cost $0.01".to_owned()];

    write_run_output(
        &mut stdout,
        &mut stderr,
        &scrubber,
        "the answer",
        &meter,
        result_notice(false),
    )
    .unwrap();

    assert_eq!(String::from_utf8(stdout).unwrap(), "the answer\n");
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        format!(
            "tokens 12 in / 7 out\ncost $0.01\n{}\n",
            result_notice(false)
        )
    );
}

type RequestLog = Arc<Mutex<Vec<ChatRequest>>>;

struct HostileReadOnlyClient {
    calls: AtomicUsize,
    requests: RequestLog,
}

impl ChatClient for HostileReadOnlyClient {
    fn complete(&self, request: &ChatRequest) -> anyhow::Result<ChatResponse> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push(request.clone());
        if call == 0 {
            return Ok(ChatResponse {
                message: ChatMessage {
                    role: "assistant".into(),
                    content: None,
                    parts: None,
                    tool_calls: Some(vec![
                        ToolCallReq {
                            id: "read".into(),
                            name: "read_file".into(),
                            arguments: r#"{"path":"note.txt"}"#.into(),
                        },
                        ToolCallReq {
                            id: "write".into(),
                            name: "write_file".into(),
                            arguments: r#"{"path":"created.txt","content":"unexpected"}"#.into(),
                        },
                        ToolCallReq {
                            id: "edit".into(),
                            name: "edit_file".into(),
                            arguments:
                                r#"{"path":"note.txt","old_string":"before","new_string":"after"}"#
                                    .into(),
                        },
                        ToolCallReq {
                            id: "exec".into(),
                            name: "exec_shell".into(),
                            arguments: r#"{"command":"echo unexpected > marker.txt"}"#.into(),
                        },
                        ToolCallReq {
                            id: "alias".into(),
                            name: "shell".into(),
                            arguments: r#"{"command":"echo unexpected > alias-marker.txt"}"#.into(),
                        },
                        ToolCallReq {
                            id: "mcp".into(),
                            name: "mcp__remote__mutate".into(),
                            arguments: "{}".into(),
                        },
                    ]),
                    tool_call_id: None,
                    reasoning_content: None,
                },
                finish_reason: FinishReason::ToolUse,
                usage: None,
                retries: Default::default(),
            });
        }
        assert_eq!(call, 1, "agent should finish after the refusal turn");
        Ok(ChatResponse {
            message: ChatMessage {
                role: "assistant".into(),
                content: Some("read-only analysis complete".into()),
                parts: None,
                tool_calls: None,
                tool_call_id: None,
                reasoning_content: None,
            },
            finish_reason: FinishReason::Stop,
            usage: None,
            retries: Default::default(),
        })
    }
}

fn captured_tool_result(request: &ChatRequest, id: &str) -> String {
    request
        .messages
        .iter()
        .find(|message| message.role == "tool" && message.tool_call_id.as_deref() == Some(id))
        .and_then(|message| message.content.clone())
        .unwrap_or_else(|| panic!("missing result for tool call {id}"))
}

#[test]
fn read_only_agent_refuses_hostile_tools_and_alias_without_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("note.txt"), "before").unwrap();
    let approvals = Arc::new(AtomicUsize::new(0));
    let approvals_seen = Arc::clone(&approvals);
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut agent = AgentLoop {
        client: Box::new(HostileReadOnlyClient {
            calls: AtomicUsize::new(0),
            requests: Arc::clone(&requests),
        }),
        tools: read_only_tools(),
        ctx: ToolCtx::new(
            dir.path().to_path_buf(),
            Box::new(move |_| {
                approvals_seen.fetch_add(1, Ordering::SeqCst);
                true
            }),
            read_only_guard(Box::new(|_| nh_tools::Guard::Allow)),
            Scrubber::new(Vec::new()),
        ),
        receipts: ReceiptWriter::for_path(
            dir.path(),
            dir.path().join("receipts.jsonl"),
            Scrubber::new(Vec::new()),
        ),
        model_id: "hostile-fixture".into(),
        max_turns: 2,
        thinking: ThinkingEffort::None,
        profile: None,
        constitution: Some(READ_ONLY_RUN_RULE.into()),
        context_limit: None,
        on_event: None,
    };

    let (answer, receipt) = agent.run("review without changing anything").unwrap();

    assert_eq!(answer, "read-only analysis complete");
    assert_eq!(receipt.tool_calls, 6);
    assert_eq!(approvals.load(Ordering::SeqCst), 0);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("note.txt")).unwrap(),
        "before"
    );
    for path in ["created.txt", "marker.txt", "alias-marker.txt"] {
        assert!(
            !dir.path().join(path).exists(),
            "unexpected mutation: {path}"
        );
    }
    assert!(dir.path().join("receipts.jsonl").exists());

    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0]
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["read_file", "glob_files", "grep_files"]
    );
    assert_eq!(captured_tool_result(&requests[1], "read"), "before");
    let available = "available tools: read_file, glob_files, grep_files";
    for id in ["write", "edit", "exec", "mcp"] {
        let result = captured_tool_result(&requests[1], id);
        assert!(result.contains("unknown tool"), "got: {result}");
        assert!(result.contains(available), "got: {result}");
    }
    let alias = captured_tool_result(&requests[1], "alias");
    assert!(
        alias.contains("mapped tool name 'shell' to 'exec_shell'"),
        "got: {alias}"
    );
    assert!(alias.contains("unknown tool 'shell'"), "got: {alias}");
    assert!(alias.contains(available), "got: {alias}");
}

#[test]
fn run_constitution_contains_the_authoritative_tool_result_rule() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let constitution = agent_constitution("law bytes", &route, false);

    assert!(
        constitution.contains(nh_core::agent::TOOL_RESULT_STATE_RULE),
        "got: {constitution}"
    );
}

#[test]
fn read_only_run_constitution_names_tools_and_honest_limits() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let normal = run_constitution("law bytes", &route, false, false, None, false);
    let read_only = run_constitution("law bytes", &route, true, false, None, false);
    let retained = run_constitution("law bytes", &route, true, true, None, false);

    assert!(!normal.contains("READ-ONLY RUN"));
    for expected in [
        "read_file, glob_files, and grep_files",
        "Do not claim to edit files",
        "sent to the selected provider",
        "provider charges",
        "local receipt writes",
    ] {
        assert!(read_only.contains(expected), "got: {read_only}");
    }
    assert!(retained.contains("read_file, glob_files, grep_files, and read_observation"));
    assert!(!read_only.contains("read_observation"));
}

#[test]
fn compact_identity_changes_only_the_identity_clause() {
    let resolver = RouteResolver::from_toml(BUNDLED_CATALOG).unwrap();
    let route = resolver.resolve("deepseek-v4-flash").unwrap();
    let standard = run_constitution("law bytes", &route, true, true, None, false);
    let compact = run_constitution(
        "law bytes",
        &route,
        true,
        true,
        Some(IdentityPromptArg::CompactV1),
        false,
    );

    let standard_suffix = standard.split_once("\n\n").unwrap().1;
    let compact_suffix = compact.split_once("\n\n").unwrap().1;
    assert_eq!(standard_suffix, compact_suffix);
    assert!(compact.starts_with(
        "You are nosis, an autonomous coding harness on route 'deepseek-v4-flash' via deepseek."
    ));
    assert!(compact.contains("answer 'nosis on deepseek-v4-flash'"));
    assert!(compact.contains(nh_core::agent::TOOL_RESULT_STATE_RULE));
    assert!(compact.contains(READ_ONLY_RUN_WITH_OBSERVATIONS_RULE));
    assert!(compact.ends_with(READ_ONLY_RUN_WITH_OBSERVATIONS_RULE));
    assert!(compact.len() < standard.len());
}

#[test]
fn shell_denied_run_constitution_has_one_factual_notice_before_the_law() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let constitution = run_constitution("law bytes", &route, false, false, None, true);

    assert_eq!(
        constitution
            .matches(nh_core::agent::SHELL_UNAVAILABLE_SESSION_NOTICE)
            .count(),
        1
    );
    assert!(constitution.ends_with("law bytes"), "got: {constitution}");
    assert!(constitution.contains("other permitted work can still complete"));
    assert!(constitution.contains("Tests require independent verification"));
    assert!(!constitution.contains("file changes can still complete"));

    let read_only = run_constitution("law bytes", &route, true, false, None, true);
    assert_eq!(
        read_only
            .matches(nh_core::agent::SHELL_UNAVAILABLE_SESSION_NOTICE)
            .count(),
        1
    );
    assert!(read_only.contains(READ_ONLY_RUN_RULE));
    assert!(!read_only.contains("file changes can still complete"));
}

#[test]
fn extractive_context_requires_recoverable_observations() {
    let error = validate_context_experiment(Some(ContextExperimentArg::ExtractiveV1), false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("requires --retain-observations"));
    validate_context_experiment(Some(ContextExperimentArg::ExtractiveV1), true).unwrap();
    validate_context_experiment(None, false).unwrap();
}

#[test]
fn timeout_guidance_never_suggests_more_than_the_cli_limit() {
    assert!(max_turns_timeout_message(60).contains("--max-turns 100"));
    let at_limit = max_turns_timeout_message(MAX_RUN_TURNS);
    assert!(at_limit.contains("split the task"), "got: {at_limit}");
    assert!(!at_limit.contains("--max-turns 200"), "got: {at_limit}");
}
