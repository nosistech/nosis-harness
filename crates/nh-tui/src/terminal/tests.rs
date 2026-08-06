use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[test]
fn terminal_guard_drop_runs_teardown() {
    let restored = Arc::new(AtomicBool::new(false));
    {
        let restored_for_guard = Arc::clone(&restored);
        let _guard = TerminalGuard::with_restore(move || {
            restored_for_guard.store(true, Ordering::SeqCst);
        });
        assert!(!restored.load(Ordering::SeqCst));
    }
    assert!(restored.load(Ordering::SeqCst));
}

#[test]
fn terminal_setup_tracks_only_modes_it_enables() {
    let mut commands = Vec::new();

    run_setup_sequence(|command| {
        commands.push(command);
        Ok(())
    })
    .unwrap();

    assert_eq!(
        commands,
        [
            SetupCommand::EnterScreen,
            SetupCommand::EnablePaste,
            SetupCommand::HideCursor,
            SetupCommand::SetTitle,
        ],
        "setup must enable only the modes owned by this TUI"
    );
}

#[test]
fn restore_failure_does_not_suppress_later_commands() {
    let mut commands = Vec::new();

    let error = run_restore_sequence(|command| {
        commands.push(command);
        if command == RestoreCommand::DisablePaste {
            Err(io::Error::other("injected first-command failure"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();

    assert_eq!(error.to_string(), "injected first-command failure");
    assert_eq!(
        commands,
        [
            RestoreCommand::ClearTitle,
            RestoreCommand::DisablePaste,
            RestoreCommand::ShowCursor,
            RestoreCommand::LeaveScreen,
        ],
        "every later inverse must run after an earlier failure"
    );
}

#[test]
fn restoration_clears_a_title_only_after_setup_owned_it() {
    let state = TerminalStateHandle::default();
    state.mark_setup(SetupCommand::SetTitle);
    let mut commands = Vec::new();

    run_owned_restore_sequence(&state, |command| {
        commands.push(command);
        Ok(())
    })
    .unwrap();

    assert_eq!(commands, [RestoreCommand::ClearTitle]);
    assert!(!state.owns_restore(RestoreCommand::ClearTitle));
}

#[test]
fn restoration_undoes_only_successfully_enabled_modes() {
    let state = TerminalStateHandle::default();
    state.mark_setup(SetupCommand::EnterScreen);
    state.mark_setup(SetupCommand::HideCursor);
    let mut commands = Vec::new();

    run_owned_restore_sequence(&state, |command| {
        commands.push(command);
        Ok(())
    })
    .unwrap();

    assert_eq!(
        commands,
        [RestoreCommand::ShowCursor, RestoreCommand::LeaveScreen]
    );
    assert!(!state.owns_restore(RestoreCommand::ShowCursor));
    assert!(!state.owns_restore(RestoreCommand::LeaveScreen));
}

#[test]
fn caught_session_restores_hook_once_before_resuming_panic() {
    let restores = AtomicUsize::new(0);
    let caught = panic::catch_unwind(AssertUnwindSafe(|| {
        let result = panic::catch_unwind(AssertUnwindSafe(|| panic!("session panic")));
        finish_caught_session(result, || {
            restores.fetch_add(1, Ordering::SeqCst);
        });
    }));

    assert!(caught.is_err());
    assert_eq!(restores.load(Ordering::SeqCst), 1);
}

#[test]
fn successful_session_restores_hook_once() {
    let restores = AtomicUsize::new(0);
    let value = finish_caught_session(Ok::<_, PanicPayload>(7), || {
        restores.fetch_add(1, Ordering::SeqCst);
    });

    assert_eq!(value, 7);
    assert_eq!(restores.load(Ordering::SeqCst), 1);
}
