//! Slash-command parsing and execution.

use super::{activate_palette_entry, UiAction};
use crate::session::{effort_for, effort_name, parse_effort};
use crate::state::{App, Overlay, PaletteEntry, PickerKind, PickerRow, TranscriptKind};
use chrono::Utc;
use nh_core::wire::UsageEvidence;
use nh_routes::{
    cost_of, money_with_gloss, to_usd_approx, Currency, PriceConfidence, ResolvedRoute, RouteClass,
};

pub(crate) fn execute_command_menu(app: &mut App) -> UiAction {
    let command_text = app.input.strip_prefix('/').unwrap_or("");
    let typed = command_text.split_whitespace().next().unwrap_or("");
    let expected = format!("/{typed}");
    let exact = app.palette_entries.iter().any(|entry| {
        entry.kind == "command" && entry.name.split_whitespace().next().unwrap_or("") == expected
    });
    if !command_text.chars().any(char::is_whitespace) && (typed.is_empty() || !exact) {
        let selected = match app.overlay {
            Overlay::CommandMenu { selected } => selected,
            _ => 0,
        };
        if let Some(entry) = command_matches(app).get(selected).copied().cloned() {
            return activate_palette_entry(app, entry);
        }
    }
    execute_command(app)
}
pub(crate) fn command_matches(app: &App) -> Vec<&PaletteEntry> {
    let query = app
        .input
        .strip_prefix('/')
        .unwrap_or("")
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_lowercase();
    app.palette_entries
        .iter()
        .filter(|entry| {
            entry.kind == "command"
                && (query.is_empty()
                    || entry.name.to_lowercase().contains(&query)
                    || entry.description.to_lowercase().contains(&query))
        })
        .collect()
}

pub(super) fn execute_command(app: &mut App) -> UiAction {
    let input = std::mem::take(&mut app.input);
    app.overlay = Overlay::None;
    let mut parts = input.strip_prefix('/').unwrap_or("").split_whitespace();
    let name = parts.next().unwrap_or("");
    let arg = parts.next();
    match (name, arg) {
        ("help" | "?", _) => {
            app.overlay = Overlay::Palette {
                filter: String::new(),
                selected: 0,
                detail: None,
            };
            UiAction::None
        }
        ("trust", _) => {
            app.overlay = Overlay::TrustDial;
            UiAction::None
        }
        ("timeline", _) => {
            app.overlay = Overlay::Timeline {
                selected: app.timeline.len().saturating_sub(1),
                inspecting: false,
                note: None,
            };
            UiAction::None
        }
        ("search", None) => {
            app.open_search();
            UiAction::None
        }
        ("search", Some(_)) => command_error(
            app,
            "/search takes no arguments",
            "run /search, then type a query",
        ),
        ("why", None) => explain_why(app),
        ("why", Some(_)) => command_error(app, "/why takes no arguments", "run /why by itself"),
        ("profile", Some(name)) => set_profile(app, name),
        ("profile", None) => open_profile_picker(app),
        ("model", Some(id)) => resolved_route_action(app, app.resolver.resolve(id)),
        ("model", None) => open_model_picker(app),
        ("provider", Some(provider)) => {
            resolved_route_action(app, app.resolver.provider_default(provider))
        }
        ("provider", None) => open_provider_picker(app),
        ("effort", Some(value)) => match parse_effort(value) {
            Some(effort) => UiAction::SetEffort(effort),
            None => command_error(
                app,
                "unknown reasoning effort",
                "use /effort <none|low|high|max>",
            ),
        },
        ("effort", None) => command_error(
            app,
            "reasoning effort is required",
            "use /effort <none|low|high|max>",
        ),
        ("quit", _) => UiAction::Quit,
        _ => command_error(app, "unknown command", "type / to see all"),
    }
}

fn open_model_picker(app: &mut App) -> UiAction {
    let rows = model_picker_rows(app);
    let selected = selected_row(&rows, app.route.id());
    app.overlay = Overlay::Picker {
        kind: PickerKind::Model,
        selected,
        rows,
    };
    UiAction::None
}

fn open_provider_picker(app: &mut App) -> UiAction {
    let rows = app
        .credentialed_providers
        .iter()
        .map(|provider| {
            let default = app.resolver.provider_default(provider).map_or_else(
                |_| "default unavailable".to_owned(),
                |route| route.id().to_owned(),
            );
            PickerRow {
                value: provider.clone(),
                label: format!("{provider} · {default} · credential available"),
            }
        })
        .collect::<Vec<_>>();
    let selected = selected_row(&rows, app.route.provider());
    app.overlay = Overlay::Picker {
        kind: PickerKind::Provider,
        selected,
        rows,
    };
    UiAction::None
}

fn open_profile_picker(app: &mut App) -> UiAction {
    let rows = ["frugal", "balanced", "max-quality"]
        .into_iter()
        .map(|name| {
            let policy = app.profiles.effective(name, &app.route);
            let cap = policy
                .output_cap
                .map_or_else(|| "route default".to_owned(), |cap| cap.to_string());
            PickerRow {
                value: name.to_owned(),
                label: format!(
                    "{name} · thinking {} · max output {cap}",
                    effort_name(effort_for(
                        policy.posture,
                        app.route.thinking_dialect(),
                        app.route.wire(),
                    ))
                ),
            }
        })
        .collect::<Vec<_>>();
    let selected = selected_row(&rows, &app.active_profile);
    app.overlay = Overlay::Picker {
        kind: PickerKind::Profile,
        selected,
        rows,
    };
    UiAction::None
}

fn selected_row(rows: &[PickerRow], value: &str) -> usize {
    rows.iter().position(|row| row.value == value).unwrap_or(0)
}

fn prior_meter(app: &App) -> Result<(u64, u64), &'static str> {
    let Some(entry) = app.timeline.last() else {
        return Ok((0, 0));
    };
    let Some(usage) = entry.usage.as_ref() else {
        return Err("prior usage unreported");
    };
    match usage.evidence {
        UsageEvidence::Measured => usage
            .cached_tokens
            .map(|cached| (usage.prompt_tokens, cached))
            .ok_or("prior cached-token evidence unreported"),
        UsageEvidence::Partial => Err("prior usage is a lower bound"),
        UsageEvidence::Unknown => Err("prior usage unknown"),
    }
}

fn model_picker_rows(app: &App) -> Vec<PickerRow> {
    let prior_meter = prior_meter(app);
    let prompt_est = prior_meter.as_ref().map_or(0, |(prompt, _)| *prompt);
    let meter_reason = prior_meter.err();
    let output_est = 1_024;
    let required = prompt_est.saturating_add(output_est);
    let ids = app.resolver.available();
    let at = Utc::now();
    let currencies = ids
        .iter()
        .filter_map(|id| {
            let route = app.resolver.resolve(id).ok()?;
            (route.class() == RouteClass::Api)
                .then(|| route.price_at(at).map(|quote| quote.currency))
                .flatten()
        })
        .collect::<Vec<_>>();
    let mixed_currency = currencies
        .first()
        .is_some_and(|first| currencies.iter().any(|currency| currency != first));

    ids.into_iter()
        .filter_map(|id| {
            let route = app.resolver.resolve(&id).ok()?;
            if route.class() == RouteClass::Local {
                return Some(PickerRow {
                    value: id.clone(),
                    label: format!("{id} · local · explicit selection only · no billed tokens"),
                });
            }
            let quote = route.price_at(at);
            let capability = if route.class() == RouteClass::Delegate {
                "unavailable: delegate".to_owned()
            } else if let Some(reason) = meter_reason {
                format!("context estimate unavailable: {reason}")
            } else if route.context().is_none() && required > 0 {
                "context unknown".to_owned()
            } else if route.context().is_some_and(|context| context < required) {
                format!("not capable: context below {required} tokens")
            } else {
                "capable".to_owned()
            };
            let price = meter_reason.map_or_else(
                || {
                    quote.as_ref().map_or_else(
                        || "price unknown".to_owned(),
                        |quote| {
                            cost_of(quote, prompt_est, 0, output_est).map_or_else(
                                || "price invalid".to_owned(),
                                |amount| {
                                    if amount == 0.0 {
                                        "free".to_owned()
                                    } else {
                                        format!(
                                            "est {}",
                                            money_with_gloss(
                                                amount,
                                                quote.currency,
                                                app.resolver.fx(),
                                                at
                                            )
                                        )
                                    }
                                },
                            )
                        },
                    )
                },
                |reason| format!("est unavailable: {reason}"),
            );
            let price_state = quote.as_ref().map_or("", |quote| {
                let fx_refuses_comparison = mixed_currency
                    && quote.currency == Currency::Cny
                    && app
                        .resolver
                        .fx()
                        .is_none_or(|fx| to_usd_approx(0.0, quote.currency, fx, at).is_none());
                if fx_refuses_comparison {
                    " · fx stale · comparison refused"
                } else if quote.confidence == PriceConfidence::VerifyLive {
                    " · price verify_live"
                } else {
                    ""
                }
            });
            Some(PickerRow {
                value: id.clone(),
                label: format!("{id} · {capability} · {price}{price_state}"),
            })
        })
        .collect()
}

pub(super) fn resolved_route_action(
    app: &mut App,
    resolved: anyhow::Result<ResolvedRoute>,
) -> UiAction {
    match resolved {
        Ok(route) if route.class() == RouteClass::Delegate => command_error(
            app,
            "delegate routes are not available here",
            "pick an api or local route with /model",
        ),
        Ok(route) => UiAction::SwitchRoute(route.id().to_owned()),
        Err(error) => command_error(app, &error.to_string(), "run /model to list routes"),
    }
}

pub(crate) fn teaching_error(cause: &str, next: &str) -> String {
    format!("{cause} - {next}")
}

pub(super) fn command_error(app: &mut App, cause: &str, next: &str) -> UiAction {
    app.push_line(&teaching_error(cause, next), TranscriptKind::Error);
    UiAction::None
}

pub(super) fn set_profile(app: &mut App, name: &str) -> UiAction {
    if !app.profiles.contains(name) {
        return command_error(
            app,
            &format!("unknown profile '{name}'"),
            "use /profile <frugal|balanced|max-quality>",
        );
    }
    let policy = app.profiles.effective(name, &app.route);
    app.active_profile = policy.profile.clone();
    app.effort = effort_for(
        policy.posture,
        app.route.thinking_dialect(),
        app.route.wire(),
    );
    let cap = policy
        .output_cap
        .map_or_else(|| "route default".to_owned(), |cap| cap.to_string());
    app.push_line(
        &format!(
            "profile {} - next turn: thinking {} · max output {}",
            policy.profile,
            effort_name(app.effort),
            cap
        ),
        TranscriptKind::Progress,
    );
    UiAction::SetProfile(policy.profile)
}

pub(crate) fn explain_why(app: &mut App) -> UiAction {
    let (prompt_est, cached_est) = match prior_meter(app) {
        Ok((prompt, cached)) => (prompt, cached.min(prompt)),
        Err(reason) => {
            return command_error(app, reason, "complete a measured turn with cache evidence")
        }
    };
    let output_est = 1_024;
    let available = app.resolver.available();
    let allowed: Vec<&str> = available
        .iter()
        .filter(|id| {
            app.resolver
                .resolve(id)
                .is_ok_and(|route| route.class() == RouteClass::Api)
        })
        .map(String::as_str)
        .collect();
    let at = Utc::now();
    let resolved = app
        .resolver
        .resolve_capable(prompt_est, output_est, &allowed, at);
    let (route, trace) = match resolved {
        Ok(result) => result,
        Err(error) => {
            return command_error(
                app,
                &error.to_string(),
                "add a priced api route with enough context",
            )
        }
    };

    app.push_line(
        &format!(
            "route: {} (cheapest capable at ~{} tokens, est)",
            route.id(),
            prompt_est.saturating_add(output_est)
        ),
        TranscriptKind::Progress,
    );
    if let Some(quote) = route.price_at(at) {
        let mut line = match cost_of(&quote, prompt_est, cached_est, output_est) {
            Some(estimate) => format!(
                "  {} this turn (est)",
                money_with_gloss(estimate, quote.currency, app.resolver.fx(), at)
            ),
            None => "  unpriced this turn (est) - meter incomplete".into(),
        };
        if quote.confidence == PriceConfidence::VerifyLive {
            line.push_str(" · *price verify_live");
        }
        app.push_line(&line, TranscriptKind::Progress);
    }
    for line in trace.lines() {
        app.push_line(&line, TranscriptKind::Progress);
    }
    if app.route.id() != route.id() {
        app.push_line(
            &format!(
                "current route {} was selected explicitly; cheapest capable is {}",
                app.route.id(),
                route.id()
            ),
            TranscriptKind::Progress,
        );
    }
    UiAction::None
}
