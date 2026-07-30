use super::*;
use chrono::TimeZone;

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
    valid_until = "2099-12-31"

    [routes.peak-route.price.peak]
    multiplier = 2.0
    timezone = "Asia/Shanghai"
    windows = ["09:00-12:00"]
"#;

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
        "model_id = \"deepseek-v4-flash\"",
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
        "mcp server \"repo-auto\": repository config cannot introduce a destination — \
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
fn yes_parsing_defaults_to_deny() {
    assert!(is_yes("y\n"));
    assert!(is_yes("  yes  \n"));
    assert!(!is_yes("\n"));
    assert!(!is_yes("n\n"));
    assert!(!is_yes("whatever\n"));
}

#[test]
fn think_flag_resolves_within_route_capability() {
    let cases = [
        (ThinkArg::None, ThinkingEffort::None),
        (ThinkArg::Low, ThinkingEffort::None),
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
    let lines = run_meter_lines(&resolver, &route, None, 2, 1, at, at);

    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("tokens: not reported by provider — cost unknown"));
    assert!(!lines[0].contains("cost $0.00"));
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
    };

    let reported = run_meter_lines(&resolver, &route, Some(&usage), 1, 0, at, at);
    assert_eq!(reported[1], nh_routes::LOCAL_METER_COPY);
    assert!(!reported.iter().any(|line| line.contains("$0.00")));

    let missing = run_meter_lines(&resolver, &route, None, 1, 0, at, at);
    assert_eq!(missing[1], nh_routes::LOCAL_METER_COPY);
}

#[test]
fn run_cost_marks_only_a_peak_boundary_crossing() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let usage = Usage {
        prompt_tokens: 100,
        completion_tokens: 50,
        cached_tokens: Some(20),
    };
    let before_peak = Utc.with_ymd_and_hms(2026, 7, 15, 0, 30, 0).unwrap();
    let in_peak = Utc.with_ymd_and_hms(2026, 7, 15, 1, 30, 0).unwrap();
    let later_in_peak = Utc.with_ymd_and_hms(2026, 7, 15, 2, 30, 0).unwrap();

    let crossed = turn_cost_line_for_run(&resolver, &route, &usage, before_peak, in_peak).unwrap();
    assert!(crossed.contains("*priced at run end — spans a peak boundary"));

    let steady = turn_cost_line_for_run(&resolver, &route, &usage, in_peak, later_in_peak).unwrap();
    assert!(!steady.contains("spans a peak boundary"));
}

#[test]
fn non_terminal_stdin_cannot_approve_even_when_it_contains_yes() {
    let mut input = std::io::Cursor::new(b"y\n".to_vec());
    let mut stderr = Vec::new();

    assert!(!approve_with_io(
        "echo safe",
        false,
        &mut input,
        &mut stderr
    ));
    assert_eq!(input.position(), 0, "piped input must not be consumed");
    let stderr = String::from_utf8(stderr).unwrap();
    assert!(stderr.contains("stdin is not a terminal"), "got: {stderr}");
    assert!(stderr.contains("cannot approve"), "got: {stderr}");
}

#[test]
fn terminal_stdin_keeps_the_existing_explicit_yes_path() {
    let mut input = std::io::Cursor::new(b"yes\n".to_vec());
    let mut stderr = Vec::new();

    assert!(approve_with_io("echo safe", true, &mut input, &mut stderr));
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        "  approve? echo safe  [y/N] "
    );
}

#[test]
fn run_stdout_contains_only_the_answer_and_metering_uses_stderr() {
    let scrubber = Scrubber::new(Vec::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let meter = vec!["tokens 12 in / 7 out".to_owned(), "cost $0.01".to_owned()];

    write_run_output(&mut stdout, &mut stderr, &scrubber, "the answer", &meter).unwrap();

    assert_eq!(String::from_utf8(stdout).unwrap(), "the answer\n");
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        "tokens 12 in / 7 out\ncost $0.01\n"
    );
}

#[test]
fn run_constitution_contains_the_authoritative_tool_result_rule() {
    let resolver = RouteResolver::from_toml(PEAK_CATALOG).unwrap();
    let route = resolver.resolve("peak-route").unwrap();
    let constitution = agent_constitution("law bytes", &route);

    assert!(
        constitution.contains(nh_core::agent::TOOL_RESULT_STATE_RULE),
        "got: {constitution}"
    );
}

#[test]
fn timeout_guidance_never_suggests_more_than_the_cli_limit() {
    assert!(max_turns_timeout_message(60).contains("--max-turns 100"));
    let at_limit = max_turns_timeout_message(MAX_RUN_TURNS);
    assert!(at_limit.contains("split the task"), "got: {at_limit}");
    assert!(!at_limit.contains("--max-turns 200"), "got: {at_limit}");
}
