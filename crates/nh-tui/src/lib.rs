//! M3 TUI: one status, one worker, and small Windows-safe views.

mod input;
mod palette;
mod render;
mod session;
mod state;
mod terminal;
mod timeline;
mod worker;

use std::cell::Cell;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{mpsc::TryRecvError, Arc, RwLock};
use std::time::Duration;

#[cfg(test)]
use std::sync::Mutex;

use anyhow::Context as _;
use chrono::{DateTime, FixedOffset, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use input::*;
use nh_core::agent::MAX_TASK_BYTES;
use nh_core::credential;
use nh_core::receipt::{FailureClass, Outcome, Receipt};
use nh_core::wire::{cache_hit_pct, resolve_effort, ChatClient, ThinkingEffort, Usage};
use nh_law::{Autonomy, Law, PolicyView};
use nh_routes::{
    cost_of, money, money_with_gloss, saved_pct, Currency, PriceConfidence, Profiles,
    ResolvedRoute, RouteClass, RouteResolver, ThinkingDialect, ThinkingPosture, Wire,
};
use nh_tools::{builtin_tools, McpAuth, McpServerConfig, McpToolset, McpTrust};
use nh_vault::{EnvFallbackVault, KeyringVault, Scrubber, SecretRegistry, SecretValue};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap},
    Frame, Terminal,
};

use palette::{builtin_palette_entries, short_text, trust_dial_lines};
pub use palette::{filter_palette, mcp_palette_entries};
use render::*;

use session::{effort_for, effort_name, install_literal, parse_effort, safe_line, scrub_full_line};
#[cfg(test)]
use session::{emit_taskbar_transition, finish_worker_shutdown};
pub use session::{identity_constitution, run};
pub use state::{
    AgentEvent, App, McpState, PaletteEntry, Status, TimelineEntry, TimelineSummary, TuiConfig,
};
use state::{Overlay, PaletteAction, TranscriptKind};
use terminal::{with_terminal_panic_hook, PanicAbort, TerminalGuard, TerminalStateHandle};

pub use timeline::apply_event;
#[cfg(test)]
use timeline::{record_turn_cost, savings_lines};
use timeline::{timeline_detail_lines, timeline_row};
pub use worker::ApprovalRequest;
use worker::{spawn_worker, Worker, WorkerCommand, WorkerConfig, WorkerShutdown, SHUTDOWN_TIMEOUT};

type SharedScrubber = Arc<RwLock<Scrubber>>;
type ConnectFn = Box<
    dyn Fn(&ResolvedRoute, Option<u64>) -> anyhow::Result<(Box<dyn ChatClient>, SecretValue)>
        + Send
        + Sync,
>;

const EVENT_POLL: Duration = Duration::from_millis(50);
const BUDGET_REASON: &str = "budget reached";
const APPROVAL_LEGEND: &str = "[y] yes  [a] always  [n]/[Esc] no";
const TASKBAR_WAITING: &[u8] = b"\x1b]9;4;4;0\x07";
const TASKBAR_CLEAR: &[u8] = b"\x1b]9;4;0;0\x07";

#[cfg(test)]
mod tests;
