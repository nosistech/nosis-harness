//! Top-level session resume command. Fleet resume remains a separate verb.

use std::io::Write;
use std::path::Path;

use nh_core::session_ledger::{
    list_sessions, read_session, validate_session_id, RestoredSession, SessionSummary, Surface,
};
use nh_routes::RouteResolver;
use nh_vault::Scrubber;

use crate::{cmd_chat, cmd_run, cmd_tui};

const DISPLAY_LIMIT: usize = 10;

pub fn run(session_id: Option<&str>) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let (root, catalog) = cmd_run::find_catalog(&cwd)?;
    let Some(session_id) = session_id else {
        return list_at(&root, &mut std::io::stdout());
    };

    validate_session_id(session_id)?;
    let restored = read_session(&root, session_id)?;
    let resolver = RouteResolver::from_toml(&catalog)?;
    validate_active_route(&resolver, &restored)?;
    match restored.surface {
        Surface::Chat => cmd_chat::resume(restored),
        Surface::Tui => cmd_tui::resume(restored),
    }
}

fn list_at(root: &Path, out: &mut dyn Write) -> anyhow::Result<()> {
    let index = list_sessions(root)?;
    let unfinished = index
        .sessions
        .iter()
        .filter(|session| !session.ended)
        .collect::<Vec<_>>();
    let scrubber = Scrubber::new(Vec::new());
    if unfinished.is_empty() {
        writeln!(
            out,
            "no interrupted sessions - start one with `nh chat` or `nh tui`"
        )?;
    } else {
        let id_width = unfinished
            .iter()
            .take(DISPLAY_LIMIT)
            .map(|session| session.session_id.len())
            .max()
            .unwrap_or(0);
        let model_width = unfinished
            .iter()
            .take(DISPLAY_LIMIT)
            .map(|session| session.model_id.len())
            .max()
            .unwrap_or(0);
        for session in unfinished.iter().take(DISPLAY_LIMIT) {
            writeln!(
                out,
                "{}",
                nh_vault::safe_line(&scrubber, &summary_line(session, id_width, model_width))
            )?;
        }
        let more = unfinished.len().saturating_sub(DISPLAY_LIMIT);
        if more > 0 {
            writeln!(
                out,
                "{more} more interrupted sessions - inspect .nosis/sessions/ for their ids"
            )?;
        }
    }
    if !index.unreadable.is_empty() {
        writeln!(
            out,
            "{} session files could not be read - inspect .nosis/sessions/",
            index.unreadable.len()
        )?;
    }
    Ok(())
}

fn summary_line(session: &SessionSummary, id_width: usize, model_width: usize) -> String {
    let surface = match session.surface {
        Surface::Chat => "chat",
        Surface::Tui => "tui",
    };
    let state = if session.ended {
        "ended cleanly"
    } else {
        "interrupted"
    };
    format!(
        "{id:<id_width$}  {surface:<4}  {model:<model_width$}  {turns:>3} turns  {created}  {state}",
        id = session.session_id,
        model = session.model_id,
        turns = session.turns,
        created = session.created_utc,
    )
}

fn validate_active_route(
    resolver: &RouteResolver,
    restored: &RestoredSession,
) -> anyhow::Result<()> {
    let route = resolver.resolve(&restored.route_id).map_err(|_| {
        anyhow::anyhow!(
            "session route {} is no longer available - restore it in catalog.toml, then retry",
            restored.route_id
        )
    })?;
    if route.model_id() != restored.model_id {
        anyhow::bail!(
            "session route {} now points to a different model - restore the recorded catalog entry, then retry",
            restored.route_id
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nh_core::session_ledger::{SessionEvent, SessionLedger};

    const ROUTES: &str = r#"
        [routes.present]
        provider = "test"
        model_id = "present-model"
        base_url = "https://example.invalid"
        wire = "openai"
        vault_entry = "test"
    "#;

    fn started(id: &str, route_id: &str, model_id: &str) -> SessionEvent {
        SessionEvent::Started {
            session_id: id.to_owned(),
            surface: Surface::Chat,
            route_id: route_id.to_owned(),
            model_id: model_id.to_owned(),
            profile: "balanced".to_owned(),
            created_utc: "2026-07-31T12:00:00Z".to_owned(),
        }
    }

    #[test]
    fn resume_route_validation_fails_closed_and_names_missing_route() {
        let restored = nh_core::session_ledger::fold_session(&[started(
            "missing-session",
            "removed-route",
            "removed-model",
        )])
        .unwrap();
        let resolver = RouteResolver::from_toml(ROUTES).unwrap();

        let error = validate_active_route(&resolver, &restored).unwrap_err();

        assert!(error.to_string().contains("removed-route"));
        assert!(error.to_string().contains("no longer available"));
        assert!(error.to_string().contains("catalog.toml"));
    }

    #[test]
    fn resume_listing_shows_only_interrupted_sessions() {
        let root = tempfile::tempdir().unwrap();
        for (id, ended) in [("open-session", false), ("closed-session", true)] {
            let ledger =
                SessionLedger::create(root.path(), id, nh_vault::Scrubber::new(Vec::new()));
            ledger
                .append(&started(id, "present", "present-model"))
                .unwrap();
            if ended {
                ledger
                    .append(&SessionEvent::Ended {
                        ts_utc: "2026-07-31T12:01:00Z".to_owned(),
                    })
                    .unwrap();
            }
        }
        let mut out = Vec::new();

        list_at(root.path(), &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();

        assert!(out.contains("open-session"));
        assert!(out.contains("interrupted"));
        assert!(!out.contains("closed-session"));
    }
}
