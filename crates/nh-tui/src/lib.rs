//! M3 TUI: one status, one worker, and small Windows-safe views.

mod input;
mod keymap;
mod palette;
mod render;
mod session;
mod state;
mod terminal;
mod timeline;
mod worker;

use std::sync::{Arc, RwLock};
use std::time::Duration;

#[cfg(test)]
use input::*;
use nh_core::wire::ChatClient;
#[cfg(test)]
use nh_core::{
    receipt::{FailureClass, Outcome, Receipt},
    wire::{ThinkingEffort, Usage, UsageEvidence},
};
#[cfg(test)]
use nh_law::{Autonomy, PolicyView};
use nh_routes::ResolvedRoute;
#[cfg(test)]
use nh_routes::{Profiles, RouteResolver};
#[cfg(test)]
use nh_tools::{McpAuth, McpServerConfig, McpToolset, McpTrust};
use nh_vault::{Scrubber, SecretValue};

#[cfg(test)]
use palette::builtin_palette_entries;
pub use palette::{filter_palette, mcp_palette_entries};
#[cfg(test)]
use render::*;

#[cfg(test)]
use session::{
    emit_taskbar_transition, finish_worker_shutdown, handle_agent_event, restore_app,
    scrub_full_line,
};
pub use session::{identity_constitution, run};
pub use state::{
    AgentEvent, App, McpState, PaletteEntry, Status, TimelineEntry, TimelineSummary, TuiConfig,
};
#[cfg(test)]
use state::{PaletteAction, TranscriptKind};

pub use timeline::apply_event;
#[cfg(test)]
use timeline::{record_turn_cost, savings_lines};
pub use worker::ApprovalRequest;
#[cfg(test)]
use worker::{spawn_worker, Worker, WorkerConfig, WorkerShutdown};

type SharedScrubber = Arc<RwLock<Scrubber>>;
type ConnectFn = Box<
    dyn Fn(&ResolvedRoute, Option<u64>) -> anyhow::Result<(Box<dyn ChatClient>, SecretValue)>
        + Send
        + Sync,
>;

const EVENT_POLL: Duration = Duration::from_millis(50);
const TURN_BELL_MIN: Duration = Duration::from_secs(10);
const BUDGET_WARN_FRACTION: (u64, u64) = (4, 5);
const BUDGET_REASON: &str = "budget reached";
const APPROVAL_LEGEND: &str = "[y] yes  [a] always  [n]/[Esc] no";
const TASKBAR_WAITING: &[u8] = b"\x1b]9;4;4;0\x07";
const TASKBAR_CLEAR: &[u8] = b"\x1b]9;4;0;0\x07";
const TITLE_ACTIVE: &[u8] = b"\x1b]0;Nosis Harness\x07";
const TITLE_IDLE: &[u8] = b"\x1b]0;Nosis Harness - IDLE\x07";
const TITLE_BLOCKED: &[u8] = b"\x1b]0;Nosis Harness - BLOCKED\x07";
const TITLE_CLEAR: &[u8] = b"\x1b]0;\x07";

#[cfg(test)]
mod tests;
