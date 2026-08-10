//! `nh profile` - keyless, read-only profile discovery for one route.

use nh_core::terminal_capability::TerminalCapability;
use nh_core::wire::effort_label;
use nh_routes::{Profiles, ResolvedRoute, RouteResolver};
use nh_vault::Scrubber;

use crate::cmd_run;

pub fn run(model: &str, terminal_capability: TerminalCapability) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (root, catalog) = cmd_run::find_catalog(&cwd)?;
    let resolver = RouteResolver::from_toml(&catalog)?;
    let route = resolver.resolve(model)?;
    let (profiles, warnings) = Profiles::load(&root);
    let scrubber = Scrubber::new(Vec::new());
    for warning in warnings {
        eprintln!("warning: {}", cmd_run::safe_line(&scrubber, &warning));
    }
    for line in render_for_terminal(terminal_capability, &profiles, &route) {
        println!("{}", cmd_run::safe_line(&scrubber, &line));
    }
    Ok(())
}

pub(crate) fn render_for_terminal(
    terminal_capability: TerminalCapability,
    profiles: &Profiles,
    route: &ResolvedRoute,
) -> Vec<String> {
    render(profiles, route)
        .into_iter()
        .map(|line| terminal_capability.render_text(&line).into_owned())
        .collect()
}

pub(crate) fn render(profiles: &Profiles, route: &ResolvedRoute) -> Vec<String> {
    profiles
        .names()
        .into_iter()
        .map(|name| {
            let policy = profiles.effective(name, route);
            let effort =
                cmd_run::effort_for(None, policy.posture, route.thinking_dialect(), route.wire());
            let cap = policy
                .output_cap
                .map_or_else(|| "route default".to_owned(), |cap| cap.to_string());
            let offpeak = if policy.prefer_offpeak {
                " · prefer off-peak"
            } else {
                ""
            };
            format!(
                "{}: thinking {} · max output {}{offpeak}",
                policy.profile,
                effort_label(effort),
                cap
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_lists_three_profiles_with_effective_caps() {
        let resolver = RouteResolver::from_toml(
            r#"
            [routes.test]
            provider = "test"
            model_id = "test"
            base_url = "https://example.invalid"
            wire = "openai"
            vault_entry = "test"
            max_out = 64000
            thinking_dialect = "deepseek-nhm"
            "#,
        )
        .unwrap();
        let route = resolver.resolve("test").unwrap();
        let lines = render(&Profiles::bundled(), &route);

        assert_eq!(lines.len(), 3);
        assert_eq!(
            lines[0],
            "frugal: thinking none · max output 16384 · prefer off-peak"
        );
        assert_eq!(lines[1], "balanced: thinking none · max output 16384");
        assert_eq!(lines[2], "max-quality: thinking high · max output 64000");
    }

    #[test]
    fn terminal_profile_render_preserves_unicode_or_uses_fallback_separators() {
        let resolver = RouteResolver::from_toml(
            r#"
            [routes.test]
            provider = "test"
            model_id = "test"
            base_url = "https://example.invalid"
            wire = "openai"
            vault_entry = "test"
            max_out = 64000
            thinking_dialect = "deepseek-nhm"
            "#,
        )
        .unwrap();
        let route = resolver.resolve("test").unwrap();

        let unicode =
            render_for_terminal(TerminalCapability::Unicode, &Profiles::bundled(), &route);
        let ascii = render_for_terminal(
            TerminalCapability::AsciiFallback,
            &Profiles::bundled(),
            &route,
        );

        assert_eq!(
            unicode[0],
            "frugal: thinking none \u{b7} max output 16384 \u{b7} prefer off-peak"
        );
        assert_eq!(
            ascii[0],
            "frugal: thinking none - max output 16384 - prefer off-peak"
        );
    }
}
