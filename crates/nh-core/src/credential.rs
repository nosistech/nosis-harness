//! The single boundary for route-scoped provider credentials and wire clients.

use crate::wire::{make_client, ChatClient};
use nh_routes::{ResolvedRoute, RouteClass};
use nh_vault::{SecretValue, Vault};

pub type CredentialedConnection = (Box<dyn ChatClient>, SecretValue);

/// Authorize a resolver-minted route, materialize only its scoped secret, and
/// build the corresponding no-redirect wire client. `output_cap` may tighten
/// the catalog cap but can never widen it.
pub fn connect<V: Vault>(
    vault: &V,
    route: &ResolvedRoute,
    approved_origins: &[String],
    output_cap: Option<u64>,
) -> anyhow::Result<CredentialedConnection> {
    connect_with_image_routes(vault, route, approved_origins, output_cap, &[])
}

/// Catalog-aware connection used by image-capable frontends. The route client
/// keeps the live catalog suggestions for its final pre-HTTP capability gate.
pub fn connect_with_catalog<V: Vault>(
    vault: &V,
    route: &ResolvedRoute,
    approved_origins: &[String],
    output_cap: Option<u64>,
    resolver: &nh_routes::RouteResolver,
) -> anyhow::Result<CredentialedConnection> {
    connect_with_image_routes(
        vault,
        route,
        approved_origins,
        output_cap,
        &resolver.routes_with_modality("image"),
    )
}

/// Connect with an already catalog-derived, sorted image-route list. This
/// keeps long-lived frontend connector closures independent of the resolver.
pub fn connect_with_image_routes<V: Vault>(
    vault: &V,
    route: &ResolvedRoute,
    approved_origins: &[String],
    output_cap: Option<u64>,
    image_capable_routes: &[String],
) -> anyhow::Result<CredentialedConnection> {
    match route.class() {
        RouteClass::Api | RouteClass::Local => {}
        RouteClass::Delegate => {
            anyhow::bail!("delegate routes do not accept provider credentials");
        }
    }
    let secret = nh_vault::get_scoped(
        vault,
        route.vault_entry(),
        route.base_url(),
        approved_origins,
    )?;
    let literal = secret.clone();
    let output_cap = min_cap(route.max_out(), output_cap);
    Ok((
        make_client(route, secret, output_cap, image_capable_routes.to_vec())?,
        literal,
    ))
}

fn min_cap(route_cap: Option<u64>, requested_cap: Option<u64>) -> Option<u64> {
    match (route_cap, requested_cap) {
        (Some(route), Some(requested)) => Some(route.min(requested)),
        (Some(route), None) => Some(route),
        (None, Some(requested)) => Some(requested),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nh_routes::RouteResolver;
    use zeroize::Zeroizing;

    struct PanicVault;

    impl Vault for PanicVault {
        fn get(&self, _entry: &str) -> anyhow::Result<Zeroizing<String>> {
            panic!("a refused route must not materialize its credential")
        }

        fn set(&self, _entry: &str, _value: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct PlaceholderVault;

    impl Vault for PlaceholderVault {
        fn get(&self, _entry: &str) -> anyhow::Result<Zeroizing<String>> {
            Ok(Zeroizing::new("ollama".to_owned()))
        }

        fn set(&self, _entry: &str, _value: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn route(base_url: &str) -> ResolvedRoute {
        RouteResolver::from_toml(&format!(
            r#"
            [routes.test]
            provider = "test"
            model_id = "test-model"
            base_url = "{base_url}"
            wire = "openai"
            vault_entry = "test"
            "#
        ))
        .unwrap()
        .resolve("test")
        .unwrap()
    }

    #[test]
    fn refuses_an_unapproved_origin_before_materializing() {
        let route = route("https://api.example.invalid:8443/v1");
        let error = connect(
            &PanicVault,
            &route,
            &["https://api.example.invalid".to_owned()],
            None,
        )
        .err()
        .expect("origin mismatch must be refused");

        assert!(error.downcast_ref::<nh_vault::AudienceRefused>().is_some());
    }

    #[test]
    fn output_cap_never_widens_the_resolved_route() {
        assert_eq!(min_cap(Some(10), Some(20)), Some(10));
        assert_eq!(min_cap(Some(20), Some(10)), Some(10));
        assert_eq!(min_cap(Some(20), None), Some(20));
        assert_eq!(min_cap(None, Some(10)), Some(10));
    }

    #[test]
    fn local_route_uses_the_existing_scoped_vault_flow() {
        let route = RouteResolver::from_toml(
            r#"
            [routes.local-test]
            provider = "ollama"
            model_id = "user-filled-model"
            base_url = "http://127.0.0.1:11434/v1"
            wire = "openai"
            vault_entry = "ollama-local"
            class = "local"
            max_out = 4096
            "#,
        )
        .unwrap()
        .resolve("local-test")
        .unwrap();

        let (_, literal) = connect(
            &PlaceholderVault,
            &route,
            &["http://127.0.0.1:11434".to_owned()],
            None,
        )
        .unwrap();
        assert_eq!(literal.as_str(), "ollama");
    }
}
