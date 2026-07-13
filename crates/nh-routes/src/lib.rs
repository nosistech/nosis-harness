//! nh-routes — RouteResolver: the ONLY component that may mint a resolved route (plan §2).
//! M0 scope: parse catalog.toml, resolve by model id, reject banned strings.
//! M1 adds: clock-aware pricing, modality dispatch, thinking dialects.

use std::collections::BTreeMap;

use anyhow::anyhow;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wire {
    OpenAi,
    AnthropicMessages,
}

#[derive(Debug, Clone)]
pub struct ResolvedRoute {
    pub provider: String,
    pub model_id: String,
    pub base_url: String,
    pub wire: Wire,
    /// nh-vault entry name; env fallback is NH_<ENTRY>_KEY.
    pub vault_entry: String,
}

/// Dead/deprecated model ids (plan §A.9). Exact ids and prefixes; `mimo-v2-` does NOT
/// match `mimo-v2.5-*` (those are current). Rejection errors must name the replacement
/// (e.g. "deepseek-chat is dead as of 2026-07-24 — use deepseek-v4-flash").
pub const BANNED_EXACT: &[&str] = &["deepseek-chat", "deepseek-reasoner"];
pub const BANNED_PREFIXES: &[&str] = &["mimo-v2-", "gpt-5.2", "gpt-5.3-codex", "moonshot-v1-"];

/// Part of the ban list: replacement to name when rejecting a banned exact id.
const BANNED_REPLACEMENTS: &[(&str, &str)] = &[
    ("deepseek-chat", "deepseek-v4-flash"),
    ("deepseek-reasoner", "deepseek-v4-pro"),
];

pub fn is_banned(model_id: &str) -> bool {
    BANNED_EXACT.contains(&model_id) || BANNED_PREFIXES.iter().any(|p| model_id.starts_with(p))
}

fn replacement_for(model_id: &str) -> Option<&'static str> {
    BANNED_REPLACEMENTS
        .iter()
        .find(|(banned, _)| *banned == model_id)
        .map(|(_, replacement)| *replacement)
}

/// Raw catalog.toml shape. Unknown keys (e.g. M1's price_confidence) are ignored
/// so newer catalog data never breaks M0.
#[derive(Deserialize)]
struct RawCatalog {
    routes: BTreeMap<String, RawRoute>,
}

#[derive(Deserialize)]
struct RawRoute {
    provider: String,
    model_id: String,
    base_url: String,
    wire: String,
    vault_entry: String,
}

pub struct RouteResolver {
    // catalog parsed from catalog.toml
    routes: BTreeMap<String, ResolvedRoute>,
}

impl RouteResolver {
    /// Parse a catalog.toml string (see repo-root catalog.toml for the schema).
    pub fn from_toml(toml_str: &str) -> anyhow::Result<Self> {
        let raw: RawCatalog = toml::from_str(toml_str)
            .map_err(|e| anyhow!("catalog.toml is invalid: {e} — fix the file and retry"))?;
        let mut routes = BTreeMap::new();
        for (id, r) in raw.routes {
            let wire = match r.wire.as_str() {
                "openai" => Wire::OpenAi,
                "anthropic" => Wire::AnthropicMessages,
                other => {
                    return Err(anyhow!(
                        "route '{id}': unknown wire '{other}' — set wire = \"openai\" or \"anthropic\" in catalog.toml"
                    ))
                }
            };
            routes.insert(
                id,
                ResolvedRoute {
                    provider: r.provider,
                    model_id: r.model_id,
                    base_url: r.base_url,
                    wire,
                    vault_entry: r.vault_entry,
                },
            );
        }
        Ok(Self { routes })
    }

    /// Resolve by model id. Banned strings error with the replacement suggestion;
    /// unknown ids error listing available routes (friendly UX, no stack traces).
    pub fn resolve(&self, model_id: &str) -> anyhow::Result<ResolvedRoute> {
        if is_banned(model_id) {
            return Err(match replacement_for(model_id) {
                Some(replacement) => anyhow!("{model_id} is dead — use {replacement}"),
                None => anyhow!(
                    "{model_id} is a dead model id — use one of: {}",
                    self.available_list()
                ),
            });
        }
        match self.routes.get(model_id) {
            Some(route) => Ok(route.clone()),
            None => Err(anyhow!(
                "unknown model id '{model_id}' — available: {}",
                self.available_list()
            )),
        }
    }

    /// All routable model ids, for `--model` help text and error messages.
    pub fn available(&self) -> Vec<String> {
        // BTreeMap keys iterate in sorted order.
        self.routes.keys().cloned().collect()
    }

    fn available_list(&self) -> String {
        if self.routes.is_empty() {
            "none (catalog.toml has no routes)".to_string()
        } else {
            self.available().join(", ")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = include_str!("../../../catalog.toml");

    fn resolver() -> RouteResolver {
        RouteResolver::from_toml(CATALOG).expect("repo-root catalog.toml must parse")
    }

    #[test]
    fn parses_repo_catalog_and_ignores_extra_keys() {
        // catalog.toml carries M1 keys (price_confidence, modality, context, valid_until);
        // from_toml must ignore them.
        let r = resolver();
        assert_eq!(r.available(), vec!["deepseek-v4-flash", "deepseek-v4-pro"]);
    }

    #[test]
    fn resolves_route_with_openai_wire() {
        let route = resolver().resolve("deepseek-v4-flash").expect("known id");
        assert_eq!(route.wire, Wire::OpenAi);
        assert_eq!(route.provider, "deepseek");
        assert_eq!(route.model_id, "deepseek-v4-flash");
        assert_eq!(route.base_url, "https://api.deepseek.com");
        assert_eq!(route.vault_entry, "deepseek");
    }

    #[test]
    fn banned_exact_ids_are_banned() {
        for id in BANNED_EXACT {
            assert!(is_banned(id), "{id} must be banned");
        }
    }

    #[test]
    fn banned_exact_errors_name_replacement() {
        let r = resolver();
        for (banned, replacement) in super::BANNED_REPLACEMENTS {
            let msg = r.resolve(banned).unwrap_err().to_string();
            assert!(msg.contains(replacement), "error for {banned} must name {replacement}: {msg}");
        }
    }

    #[test]
    fn each_banned_prefix_matches() {
        for prefix in BANNED_PREFIXES {
            let id = format!("{prefix}0test");
            assert!(is_banned(&id), "{id} must be banned");
            assert!(is_banned(prefix), "bare prefix {prefix} must be banned");
        }
    }

    #[test]
    fn banned_prefix_resolve_gives_generic_error_listing_routes() {
        let r = resolver();
        let id = format!("{}0test", BANNED_PREFIXES[0]);
        let msg = r.resolve(&id).unwrap_err().to_string();
        assert!(msg.contains("dead"), "must say the id is dead: {msg}");
        assert!(msg.contains("deepseek-v4-flash"), "must list routes: {msg}");
    }

    #[test]
    fn mimo_v2_5_pro_is_allowed() {
        assert!(!is_banned("mimo-v2.5-pro"));
        assert!(!is_banned("mimo-v2.5"));
    }

    #[test]
    fn unknown_id_error_lists_available_routes() {
        let msg = resolver().resolve("no-such-model").unwrap_err().to_string();
        assert!(msg.contains("no-such-model"), "must echo the bad id: {msg}");
        assert!(msg.contains("deepseek-v4-flash"), "must list routes: {msg}");
        assert!(msg.contains("deepseek-v4-pro"), "must list routes: {msg}");
    }

    #[test]
    fn unknown_wire_is_rejected() {
        let toml = r#"
            [routes.some-model]
            provider = "p"
            model_id = "some-model"
            base_url = "https://example.invalid"
            wire = "carrier-pigeon"
            vault_entry = "p"
        "#;
        let msg = RouteResolver::from_toml(toml).err().expect("must fail").to_string();
        assert!(msg.contains("carrier-pigeon"), "must name the bad wire: {msg}");
        assert!(msg.contains("openai"), "must say valid values: {msg}");
    }

    #[test]
    fn invalid_toml_is_a_friendly_error() {
        let msg = RouteResolver::from_toml("not [ valid").err().expect("must fail").to_string();
        assert!(msg.contains("catalog.toml is invalid"), "friendly message: {msg}");
    }
}
