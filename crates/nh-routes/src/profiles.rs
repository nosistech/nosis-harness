//! Layered execution profiles for one already-resolved route.
//!
//! Profiles never select or widen a route. They only clamp its output cap and
//! describe the thinking posture that nh-core resolves against route capability.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::anyhow;
use serde::de::{self, Deserializer};
use serde::Deserialize;

use crate::ResolvedRoute;

/// Bundled frugal, balanced, and max-quality profile data.
pub const BUNDLED_PROFILES: &str = include_str!("bundled_profiles.toml");

const REPO_WARNING: &str =
    "repo .nosis/profiles.toml cannot loosen profile '{name}' - clamped to the user setting";

/// Requested thinking position within a route's immutable capability range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThinkingPosture {
    Floor,
    Default,
    Ceiling,
}

impl ThinkingPosture {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Floor => "floor",
            Self::Default => "default",
            Self::Ceiling => "ceiling",
        }
    }
}

impl fmt::Display for ThinkingPosture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ThinkingPosture {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "floor" => Ok(Self::Floor),
            "default" => Ok(Self::Default),
            "ceiling" => Ok(Self::Ceiling),
            other => Err(de::Error::custom(format!(
                "unknown thinking posture '{other}' - use floor, default, or ceiling"
            ))),
        }
    }
}

/// One profile definition from a TOML layer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Profile {
    pub thinking: ThinkingPosture,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub prefer_offpeak: Option<bool>,
}

/// Parsed `profiles.toml` layer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ProfilesLayer {
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

impl ProfilesLayer {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        toml::from_str(text).map_err(|error| anyhow!("profiles.toml is invalid: {error}"))
    }
}

/// Compiled profiles for one session.
#[derive(Debug, Clone)]
pub struct Profiles {
    profiles: BTreeMap<String, Profile>,
}

/// Concrete profile policy for one resolved route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveExecutionPolicy {
    pub profile: String,
    pub output_cap: Option<u64>,
    pub posture: ThinkingPosture,
    pub prefer_offpeak: bool,
}

impl Profiles {
    /// Compile parsed bundled, user, and repository layers. User entries replace
    /// bundled entries. Repository entries may replace them only where every
    /// changed lever is equally or more restrictive.
    pub fn compile(
        bundled: ProfilesLayer,
        user: Option<ProfilesLayer>,
        repo: Option<ProfilesLayer>,
    ) -> (Self, Vec<String>) {
        let mut profiles = bundled.profiles;
        if let Some(user) = user {
            profiles.extend(user.profiles);
        }

        let mut warnings = Vec::new();
        if let Some(repo) = repo {
            for (name, mut candidate) in repo.profiles {
                let Some(baseline) = profiles.get(&name).cloned() else {
                    warnings.push(format!(
                        "repo .nosis/profiles.toml cannot add profile '{name}' - ignored"
                    ));
                    continue;
                };
                let mut loosened = false;
                if candidate.thinking > baseline.thinking {
                    candidate.thinking = baseline.thinking;
                    loosened = true;
                }
                if cap_is_looser(candidate.max_output_tokens, baseline.max_output_tokens) {
                    candidate.max_output_tokens = baseline.max_output_tokens;
                    loosened = true;
                }
                if baseline.prefer_offpeak == Some(true) && candidate.prefer_offpeak != Some(true) {
                    candidate.prefer_offpeak = Some(true);
                    loosened = true;
                }
                if loosened {
                    warnings.push(REPO_WARNING.replace("{name}", &name));
                }
                profiles.insert(name, candidate);
            }
        }

        (Self { profiles }, warnings)
    }

    /// Parse and compile only the embedded defaults.
    pub fn bundled() -> Self {
        let layer = ProfilesLayer::parse(BUNDLED_PROFILES)
            .expect("bundled_profiles.toml must be valid profile data");
        Self::compile(layer, None, None).0
    }

    /// Load bundled → user (`~/.nosis`) → repository (`.nosis`) profile data.
    /// Optional unreadable or malformed layers warn and are skipped.
    pub fn load(repo_root: &Path) -> (Self, Vec<String>) {
        let mut warnings = Vec::new();
        let bundled = match ProfilesLayer::parse(BUNDLED_PROFILES) {
            Ok(layer) => layer,
            Err(_) => {
                warnings.push("bundled profiles are malformed - balanced defaults kept".to_owned());
                ProfilesLayer::default()
            }
        };
        let user = match home_dir() {
            Some(home) => read_optional_profiles(
                &home.join(".nosis").join("profiles.toml"),
                "user profiles",
                &mut warnings,
            ),
            None => {
                warnings.push("home directory not found - user profiles skipped".to_owned());
                None
            }
        };
        let repo = read_optional_profiles(
            &repo_root.join(".nosis").join("profiles.toml"),
            "repo .nosis/profiles.toml",
            &mut warnings,
        );
        let (profiles, compile_warnings) = Self::compile(bundled, user, repo);
        warnings.extend(compile_warnings);
        (profiles, warnings)
    }

    /// Resolve one named profile against a route. Unknown names fall back to
    /// balanced; callers compare the returned name and render a warning.
    pub fn effective(&self, name: &str, route: &ResolvedRoute) -> EffectiveExecutionPolicy {
        let (profile_name, profile) = self
            .profiles
            .get(name)
            .map(|profile| (name, profile))
            .or_else(|| {
                self.profiles
                    .get("balanced")
                    .map(|profile| ("balanced", profile))
            })
            .unwrap_or_else(|| {
                static SAFE_BALANCED: Profile = Profile {
                    thinking: ThinkingPosture::Default,
                    max_output_tokens: None,
                    prefer_offpeak: None,
                };
                ("balanced", &SAFE_BALANCED)
            });
        let output_cap = match (route.max_out, profile.max_output_tokens) {
            (Some(route_cap), Some(profile_cap)) => Some(route_cap.min(profile_cap)),
            (Some(route_cap), None) => Some(route_cap),
            (None, Some(profile_cap)) => Some(profile_cap),
            (None, None) => None,
        };
        EffectiveExecutionPolicy {
            profile: profile_name.to_owned(),
            output_cap,
            posture: profile.thinking,
            prefer_offpeak: profile.prefer_offpeak.unwrap_or(false),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.profiles.contains_key(name)
    }

    /// Stable UX order for the built-ins, followed by any user-defined names.
    pub fn names(&self) -> Vec<&str> {
        let mut names = Vec::new();
        for built_in in ["frugal", "balanced", "max-quality"] {
            if self.profiles.contains_key(built_in) {
                names.push(built_in);
            }
        }
        let extras: Vec<&str> = self
            .profiles
            .keys()
            .map(String::as_str)
            .filter(|name| !names.contains(name))
            .collect();
        names.extend(extras);
        names
    }
}

impl EffectiveExecutionPolicy {
    /// Clone a route and clamp only its output cap for the existing client
    /// factory. Route identity, capability, pricing, and law remain unchanged.
    pub fn clamp_route(&self, route: &ResolvedRoute) -> ResolvedRoute {
        let mut clamped = route.clone();
        clamped.max_out = match self.output_cap {
            Some(cap) => Some(route.max_out.map_or(cap, |route_cap| route_cap.min(cap))),
            None => route.max_out,
        };
        clamped
    }
}

fn cap_is_looser(candidate: Option<u64>, baseline: Option<u64>) -> bool {
    match (candidate, baseline) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(candidate), Some(baseline)) => candidate > baseline,
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    const HOME_ENV: &str = "USERPROFILE";
    #[cfg(not(windows))]
    const HOME_ENV: &str = "HOME";

    std::env::var_os(HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn read_optional_profiles(
    path: &Path,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<ProfilesLayer> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => {
            warnings.push(format!("could not read {label} - source skipped"));
            return None;
        }
    };
    match ProfilesLayer::parse(&text) {
        Ok(layer) => Some(layer),
        Err(_) => {
            warnings.push(format!("could not parse {label} - defaults kept"));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RouteClass, RouteResolver, ThinkingDialect, Wire};

    fn layer(text: &str) -> ProfilesLayer {
        ProfilesLayer::parse(text).unwrap()
    }

    fn route(max_out: u64) -> ResolvedRoute {
        ResolvedRoute {
            id: "test".into(),
            provider: "test".into(),
            model_id: "test-model".into(),
            base_url: "https://example.invalid".into(),
            wire: Wire::OpenAi,
            vault_entry: "test".into(),
            class: RouteClass::Api,
            modality: vec!["text".into()],
            context: Some(100_000),
            max_out: Some(max_out),
            thinking_dialect: ThinkingDialect::DeepseekNhm,
            preserve_reasoning: true,
            preserve_when_thinking: true,
            quirks: vec!["test-quirk".into()],
            price: None,
        }
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
    fn frugal_cap_is_below_balanced_native_cap() {
        let profiles = Profiles::bundled();
        let r = route(64_000);

        assert_eq!(profiles.effective("frugal", &r).output_cap, Some(16_384));
        assert_eq!(profiles.effective("balanced", &r).output_cap, r.max_out);
    }

    #[test]
    fn clamp_route_changes_only_max_out() {
        let profiles = Profiles::bundled();
        let r = route(64_000);
        let clamped = profiles.effective("frugal", &r).clamp_route(&r);

        assert_eq!(clamped.max_out, Some(16_384));
        let mut restored = clamped;
        restored.max_out = r.max_out;
        assert_eq!(format!("{restored:?}"), format!("{r:?}"));
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
    fn unknown_profile_falls_back_to_balanced() {
        let policy = Profiles::bundled().effective("missing", &route(64_000));
        assert_eq!(policy.profile, "balanced");
        assert_eq!(policy.posture, ThinkingPosture::Default);
        assert_eq!(policy.output_cap, Some(64_000));
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
}
