use super::*;
use crate::RouteResolver;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct TempTree {
    path: PathBuf,
}

impl TempTree {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "nh-routes-profiles-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create test directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create test parent");
        }
        fs::write(path, contents).expect("write test file");
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
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

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

fn must_symlink_file(target: &Path, link: &Path) {
    symlink_file(target, link).unwrap_or_else(|error| {
        panic!(
            "symlink creation is required for this security test; enable Windows Developer Mode or run with symlink privilege: {error}"
        )
    });
}

fn must_symlink_dir(target: &Path, link: &Path) {
    symlink_dir(target, link).unwrap_or_else(|error| {
        panic!(
            "directory symlink creation is required for this security test; enable Windows Developer Mode or run with symlink privilege: {error}"
        )
    });
}

fn layer(text: &str) -> ProfilesLayer {
    ProfilesLayer::parse(text).unwrap()
}

fn route(max_out: u64) -> ResolvedRoute {
    RouteResolver::from_toml(&format!(
        r#"
        [routes.test]
        provider = "test"
        model_id = "test-model"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
        context = 100000
        max_out = {max_out}
        thinking_dialect = "deepseek-nhm"
        preserve_reasoning = true
        preserve_when_thinking = true
        quirks = ["test-quirk"]
        "#
    ))
    .unwrap()
    .resolve("test")
    .unwrap()
}

#[test]
fn compile_clamps_repo_loosen_and_keeps_repo_tightening() {
    let bundled = layer(BUNDLED_PROFILES);
    let user = layer(
        r#"
        [profiles.frugal]
        thinking = "floor"
        max_output_tokens = 12000
        prefer_offpeak = true

        [profiles.balanced]
        thinking = "default"
        max_output_tokens = 24000
        "#,
    );
    let repo = layer(
        r#"
        [profiles.frugal]
        thinking = "ceiling"
        max_output_tokens = 50000

        [profiles.balanced]
        thinking = "floor"
        max_output_tokens = 8000
        prefer_offpeak = true
        "#,
    );

    let (profiles, warnings) = Profiles::compile(bundled, Some(user), Some(repo));
    let r = route(100_000);
    let frugal = profiles.effective("frugal", &r);
    assert_eq!(frugal.posture, ThinkingPosture::Floor);
    assert_eq!(frugal.output_cap, Some(12_000));
    assert!(frugal.prefer_offpeak);
    let balanced = profiles.effective("balanced", &r);
    assert_eq!(balanced.posture, ThinkingPosture::Floor);
    assert_eq!(balanced.output_cap, Some(8_000));
    assert!(balanced.prefer_offpeak);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("cannot loosen profile 'frugal'"));
}

#[test]
fn bundled_profiles_bound_default_output_and_require_opt_in_for_more() {
    let profiles = Profiles::bundled();
    let r = route(64_000);

    assert_eq!(profiles.effective("frugal", &r).output_cap, Some(16_384));
    assert_eq!(profiles.effective("balanced", &r).output_cap, Some(16_384));
    assert_eq!(
        profiles.effective("max-quality", &r).output_cap,
        r.max_out()
    );
}

#[test]
fn effective_policy_clamps_without_minting_a_route() {
    let profiles = Profiles::bundled();
    let r = route(64_000);
    let policy = profiles.effective("frugal", &r);

    assert_eq!(policy.output_cap, Some(16_384));
    assert_eq!(r.max_out(), Some(64_000));
}

#[test]
fn min_cap_covers_the_full_option_table() {
    assert_eq!(min_cap(Some(20), Some(10)), Some(10));
    assert_eq!(min_cap(Some(10), None), Some(10));
    assert_eq!(min_cap(None, Some(10)), Some(10));
    assert_eq!(min_cap(None, None), None);
}

#[test]
fn unknown_posture_is_a_clear_error() {
    let error = ProfilesLayer::parse(
        r#"
        [profiles.frugal]
        thinking = "expensive"
        "#,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("unknown thinking posture 'expensive'"));
    assert!(error.contains("floor, default, or ceiling"));
}

#[test]
fn optional_profile_warning_preserves_the_actionable_parse_error() {
    let path = std::env::temp_dir().join(format!(
        "nh-routes-profiles-warning-{}.toml",
        std::process::id()
    ));
    fs::write(
        &path,
        r#"
        [profiles.frugal]
        thinking = "expensive"
        "#,
    )
    .unwrap();
    let mut warnings = Vec::new();

    assert!(read_optional_profiles(&path, None, "test profiles", &mut warnings).is_none());
    fs::remove_file(&path).unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("unknown thinking posture 'expensive'"));
    assert!(warnings[0].contains("defaults kept"));
}

#[test]
fn symlinked_repo_profiles_are_refused_and_user_and_bundled_layers_remain() {
    let repo = TempTree::new("repo-symlink");
    let home = TempTree::new("repo-symlink-home");
    let outside = TempTree::new("repo-symlink-outside");
    home.write(
        ".nosis/profiles.toml",
        r#"
        [profiles.operator]
        thinking = "floor"
        max_output_tokens = 1234
        "#,
    );
    outside.write(
        "profiles.toml",
        r#"
        [profiles.frugal]
        thinking = "floor"
        max_output_tokens = 1
        prefer_offpeak = true
        "#,
    );
    fs::create_dir_all(repo.path().join(".nosis")).unwrap();
    must_symlink_file(
        &outside.path().join("profiles.toml"),
        &repo.path().join(".nosis").join("profiles.toml"),
    );

    let (profiles, warnings) = Profiles::load_with_home(repo.path(), Some(home.path()));
    let resolved = route(100_000);

    assert_eq!(
        profiles.effective("operator", &resolved).output_cap,
        Some(1234)
    );
    assert_eq!(
        profiles.effective("frugal", &resolved).output_cap,
        Some(16_384)
    );
    assert_eq!(
        profiles.effective("balanced", &resolved).output_cap,
        Some(16_384)
    );
    assert_eq!(warnings.len(), 1, "got: {warnings:?}");
    assert!(warnings[0].contains("repo .nosis/profiles.toml"));
    assert!(warnings[0].contains("not a regular file"));
}

#[test]
fn oversized_repo_profiles_are_refused_before_parsing() {
    let repo = TempTree::new("repo-oversized");
    let home = TempTree::new("repo-oversized-home");
    repo.write(".nosis/profiles.toml", &" ".repeat(MAX_PROFILES_BYTES + 1));

    let (profiles, warnings) = Profiles::load_with_home(repo.path(), Some(home.path()));

    assert!(profiles.contains("balanced"));
    assert_eq!(warnings.len(), 1, "got: {warnings:?}");
    assert!(warnings[0].contains("repo .nosis/profiles.toml"));
    assert!(warnings[0].contains("exceeds 65536 bytes"));
}

#[test]
fn profiles_below_a_parent_symlink_outside_repo_are_refused() {
    let repo = TempTree::new("parent-symlink");
    let home = TempTree::new("parent-symlink-home");
    let outside = TempTree::new("parent-symlink-outside");
    outside.write(
        "profiles.toml",
        r#"
        [profiles.frugal]
        thinking = "floor"
        max_output_tokens = 1
        prefer_offpeak = true
        "#,
    );
    must_symlink_dir(outside.path(), &repo.path().join(".nosis"));

    let (profiles, warnings) = Profiles::load_with_home(repo.path(), Some(home.path()));

    assert_eq!(
        profiles.effective("frugal", &route(100_000)).output_cap,
        Some(16_384)
    );
    assert_eq!(warnings.len(), 1, "got: {warnings:?}");
    assert!(warnings[0].contains("resolves outside the repository"));
}

#[test]
fn user_global_profiles_plain_file_loads_without_repo_containment() {
    let repo = TempTree::new("user-global");
    let home = TempTree::new("user-global-home");
    home.write(
        ".nosis/profiles.toml",
        r#"
        [profiles.operator]
        thinking = "floor"
        max_output_tokens = 4321
        "#,
    );

    let (profiles, warnings) = Profiles::load_with_home(repo.path(), Some(home.path()));

    assert_eq!(
        profiles.effective("operator", &route(100_000)).output_cap,
        Some(4321)
    );
    assert!(warnings.is_empty(), "got: {warnings:?}");
}

#[test]
fn unknown_profile_falls_back_to_balanced() {
    let policy = Profiles::bundled().effective("missing", &route(64_000));
    assert_eq!(policy.profile, "balanced");
    assert_eq!(policy.posture, ThinkingPosture::Default);
    assert_eq!(policy.output_cap, Some(16_384));
}

#[test]
fn bundled_profiles_parse_independently_of_catalog() {
    let resolver = RouteResolver::from_toml(
        r#"
        [routes.test]
        provider = "test"
        model_id = "test"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
        max_out = 64000
        "#,
    )
    .unwrap();
    let route = resolver.resolve("test").unwrap();
    assert_eq!(
        Profiles::bundled().effective("max-quality", &route).posture,
        ThinkingPosture::Ceiling
    );
}
