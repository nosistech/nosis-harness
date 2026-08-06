use std::any::Any;
use std::io::{self, Write};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use anyhow::Context as _;
use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use super::{TASKBAR_CLEAR, TITLE_ACTIVE, TITLE_CLEAR};

pub(super) struct PanicAbort(AtomicBool);

impl PanicAbort {
    pub(super) fn requested(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Default)]
struct TerminalState {
    raw_mode: bool,
    alternate_screen: bool,
    bracketed_paste: bool,
    cursor_hidden: bool,
    title_set: bool,
}

#[derive(Clone, Default)]
pub(super) struct TerminalStateHandle(Arc<Mutex<TerminalState>>);

impl TerminalStateHandle {
    fn with<R>(&self, inspect: impl FnOnce(&TerminalState) -> R) -> R {
        let state = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        inspect(&state)
    }

    fn with_mut<R>(&self, update: impl FnOnce(&mut TerminalState) -> R) -> R {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        update(&mut state)
    }

    fn mark_raw_mode(&self) {
        self.with_mut(|state| state.raw_mode = true);
    }

    fn raw_mode_enabled(&self) -> bool {
        self.with(|state| state.raw_mode)
    }

    fn clear_raw_mode(&self) {
        self.with_mut(|state| state.raw_mode = false);
    }

    fn mark_setup(&self, command: SetupCommand) {
        self.with_mut(|state| match command {
            SetupCommand::EnterScreen => state.alternate_screen = true,
            SetupCommand::EnablePaste => state.bracketed_paste = true,
            SetupCommand::HideCursor => state.cursor_hidden = true,
            SetupCommand::SetTitle => state.title_set = true,
        });
    }

    fn owns_restore(&self, command: RestoreCommand) -> bool {
        self.with(|state| match command {
            RestoreCommand::DisablePaste => state.bracketed_paste,
            RestoreCommand::ShowCursor => state.cursor_hidden,
            RestoreCommand::LeaveScreen => state.alternate_screen,
            RestoreCommand::ClearTitle => state.title_set,
        })
    }

    fn clear_restore(&self, command: RestoreCommand) {
        self.with_mut(|state| match command {
            RestoreCommand::DisablePaste => state.bracketed_paste = false,
            RestoreCommand::ShowCursor => state.cursor_hidden = false,
            RestoreCommand::LeaveScreen => state.alternate_screen = false,
            RestoreCommand::ClearTitle => state.title_set = false,
        });
    }
}

pub(super) struct TerminalGuard {
    restore: Option<Box<dyn FnMut()>>,
}

impl TerminalGuard {
    pub(super) fn enter(state: TerminalStateHandle) -> anyhow::Result<Self> {
        if let Err(error) = enable_raw_mode() {
            let _ = restore_terminal(&state);
            return Err(error).context("could not enable terminal raw mode");
        }
        state.mark_raw_mode();

        let mut stdout = io::stdout();
        if let Err(error) = write_setup_commands(&mut stdout, &state) {
            let _ = restore_terminal(&state);
            return Err(error).context("could not enter the alternate screen");
        }

        let restore_state = state.clone();
        Ok(Self {
            restore: Some(Box::new(move || {
                let _ = restore_terminal(&restore_state);
            })),
        })
    }

    #[cfg(test)]
    fn with_restore(restore: impl FnMut() + 'static) -> Self {
        Self {
            restore: Some(Box::new(restore)),
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if let Some(mut restore) = self.restore.take() {
            restore();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetupCommand {
    EnterScreen,
    EnablePaste,
    HideCursor,
    SetTitle,
}

fn run_setup_sequence(mut run: impl FnMut(SetupCommand) -> io::Result<()>) -> io::Result<()> {
    for command in [
        SetupCommand::EnterScreen,
        SetupCommand::EnablePaste,
        SetupCommand::HideCursor,
        SetupCommand::SetTitle,
    ] {
        run(command)?;
    }
    Ok(())
}

fn write_setup_commands(writer: &mut impl Write, state: &TerminalStateHandle) -> io::Result<()> {
    run_setup_sequence(|command| {
        match command {
            SetupCommand::EnterScreen => execute!(writer, EnterAlternateScreen)?,
            SetupCommand::EnablePaste => execute!(writer, EnableBracketedPaste)?,
            SetupCommand::HideCursor => execute!(writer, Hide)?,
            SetupCommand::SetTitle => writer.write_all(TITLE_ACTIVE)?,
        }
        state.mark_setup(command);
        Ok(())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestoreCommand {
    DisablePaste,
    ShowCursor,
    LeaveScreen,
    ClearTitle,
}

fn retain_first(first: &mut Option<io::Error>, result: io::Result<()>) {
    if let Err(error) = result {
        if first.is_none() {
            *first = Some(error);
        }
    }
}

fn finish_restore(first: Option<io::Error>) -> io::Result<()> {
    first.map_or(Ok(()), Err)
}

fn run_restore_sequence(mut run: impl FnMut(RestoreCommand) -> io::Result<()>) -> io::Result<()> {
    let mut first = None;
    for command in [
        RestoreCommand::ClearTitle,
        RestoreCommand::DisablePaste,
        RestoreCommand::ShowCursor,
        RestoreCommand::LeaveScreen,
    ] {
        retain_first(&mut first, run(command));
    }
    finish_restore(first)
}

fn run_owned_restore_sequence(
    state: &TerminalStateHandle,
    mut run: impl FnMut(RestoreCommand) -> io::Result<()>,
) -> io::Result<()> {
    run_restore_sequence(|command| {
        if !state.owns_restore(command) {
            return Ok(());
        }
        let result = run(command);
        if result.is_ok() {
            state.clear_restore(command);
        }
        result
    })
}

fn write_restore_commands(writer: &mut impl Write, state: &TerminalStateHandle) -> io::Result<()> {
    let mut first = None;
    retain_first(
        &mut first,
        run_owned_restore_sequence(state, |command| match command {
            RestoreCommand::ClearTitle => writer.write_all(TITLE_CLEAR),
            RestoreCommand::DisablePaste => execute!(writer, DisableBracketedPaste),
            RestoreCommand::ShowCursor => execute!(writer, Show),
            RestoreCommand::LeaveScreen => execute!(writer, LeaveAlternateScreen),
        }),
    );
    retain_first(&mut first, writer.flush());
    finish_restore(first)
}

fn restore_terminal(state: &TerminalStateHandle) -> io::Result<()> {
    let mut first = None;
    if state.raw_mode_enabled() {
        let result = disable_raw_mode();
        if result.is_ok() {
            state.clear_raw_mode();
        }
        retain_first(&mut first, result);
    }

    let mut stdout = io::stdout();
    retain_first(&mut first, stdout.write_all(TASKBAR_CLEAR));
    retain_first(&mut first, write_restore_commands(&mut stdout, state));
    finish_restore(first)
}

type PanicPayload = Box<dyn Any + Send + 'static>;
type PanicHook = Box<dyn Fn(&panic::PanicHookInfo<'_>) + Send + Sync + 'static>;

struct PanicHookGuard {
    previous: Arc<Mutex<Option<PanicHook>>>,
    abort: Arc<PanicAbort>,
}

impl PanicHookGuard {
    fn install(state: TerminalStateHandle) -> Self {
        let previous = Arc::new(Mutex::new(Some(panic::take_hook())));
        let hook_previous = Arc::clone(&previous);
        let abort = Arc::new(PanicAbort(AtomicBool::new(false)));
        let hook_abort = Arc::clone(&abort);
        panic::set_hook(Box::new(move |info| {
            hook_abort.0.store(true, Ordering::Release);
            let _ = restore_terminal(&state);
            let guard = hook_previous
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(previous) = guard.as_ref() {
                previous(info);
            }
        }));
        Self { previous, abort }
    }

    fn restore(&mut self) {
        let previous = self
            .previous
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(previous) = previous {
            drop(panic::take_hook());
            panic::set_hook(previous);
        }
    }
}

impl Drop for PanicHookGuard {
    fn drop(&mut self) {
        self.restore();
    }
}

fn finish_caught_session<T>(result: Result<T, PanicPayload>, restore_hook: impl FnOnce()) -> T {
    restore_hook();
    match result {
        Ok(value) => value,
        Err(payload) => panic::resume_unwind(payload),
    }
}

pub(super) fn with_terminal_panic_hook<T>(
    session: impl FnOnce(&PanicAbort, TerminalStateHandle) -> T,
) -> T {
    let state = TerminalStateHandle::default();
    let mut hook = PanicHookGuard::install(state.clone());
    let abort = Arc::clone(&hook.abort);
    let result = panic::catch_unwind(AssertUnwindSafe(|| session(&abort, state)));
    finish_caught_session(result, || hook.restore())
}

#[cfg(test)]
mod tests;
