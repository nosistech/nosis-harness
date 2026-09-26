# Use the terminal interface

`nh chat --model <route-id>` gives a plain scrolling conversation. `nh tui --model
<route-id>` opens the full-screen interface. Replace the route placeholder with an
ID from your trusted catalog, such as the one selected during `nh setup`.

The full-screen interface shows the selected route, current activity, conversation
and usage. Reported token totals and catalog-derived money totals can be incomplete.
`--budget` stops new work based on observed usage; it is not a hard spending cap.
If a call's usage is unknown or incomplete, a budgeted session stops further
dispatch. Repeating the task in that session will not clear the stop. Review the
result and usage first, then start a new session with a deliberate budget choice.

**Unreleased source:** the header shows the project name when space permits; F1
shows the project folder. The shortcut bar fits the window, keeping Help and
the current send, approval or stop action ahead of less-used shortcuts. Help can
scroll with Up/Down or PageUp/PageDown; Home/End move to its beginning/end. Resizing
keeps the scroll position within the available help text.
Press F1 again to close Help and return to the task or pending approval without
stopping it. Help does not accept approval choices while the request is hidden.

While work is active, Esc stops the current turn, including when Help or another
overlay is open. When idle, Esc closes the overlay. On-screen Escape hints reflect
the current state.

## Write and send

**v0.3.0-rc.1 candidate:** the full-screen composer preserves pasted line breaks. Paste does
not submit the message. Review it, then press Enter to send. Shift+Enter inserts a
line break; Ctrl+J is the fallback for terminals that cannot distinguish Shift+Enter.
Some terminals report Shift+Enter as ordinary Enter and send the message; use Ctrl+J
if you are unsure how your terminal handles that key combination.
Plain chat still uses one input line at a time.

The composer displays up to five logical lines and scrolls as you edit. Left/Right
move the cursor; Home/End move within the current line; Backspace/Delete edit at the
cursor. Up/Down move between lines in a multiline message. The same byte limit
applies to pasted and typed input. Check the final prompt before sending, especially
after a paste that exceeds the limit.
While the draft has multiple lines, use PageUp/PageDown to scroll the transcript;
Up/Down are reserved for the composer.

## Know what is happening

Responses currently arrive when the provider completes its reply; generated words
are not streamed live. Waiting is not proof the app has frozen. A reasoning request
can take several minutes. Esc requests interruption of active work; cancellation
can wait for an in-flight request and does not undo completed file or command effects.

When a command needs approval, read its full text and choose deliberately. An
approval request is waiting for you, not the provider. Declining is a normal action.
Do not approve a command just to dismiss the prompt.

**Unreleased source:** approval requests wrap instead of being shortened to one
line. Use PageUp/PageDown to read a long request; Up/Down also scroll when the draft
has one line. These navigation keys do not approve anything. **F2 approves once;
F4 declines.** When offered, **F3** permits the identical shell command again
during this session. Repeat is unavailable for file actions, remote MCP requests
or commands whose text contains redacted or escaped information; choose F2 or F4
instead. Esc declines and stops the turn. Repeated approval does not authorize
different command text or carry into a new session, and it does not guarantee
that repeating a command has the same effects on changed files.

You can keep writing or pasting your next message while an approval is pending.
Ordinary letters, including `y`, `a` and `n`, stay in the draft. Enter queues that
draft for after the current turn; it never approves the request. Read the request
before pressing an approval key. Press and release the key for each choice;
terminals can turn a held key into multiple presses. Some laptops require holding Fn to send F1-F4;
check the key labels and your keyboard's function-key setting.

These function keys apply to the unreleased full-screen interface. In the current
source, plain `nh run` and `nh chat` show a fresh eight-character confirmation code
beside each approval request. Read the request, type that exact uppercase code and
press Enter to approve. Enter alone, `y`, `yes`, an old code or any other answer
declines. The code is a confirmation for that request, not a password or login.
This prevents pretyped affirmative words from authorizing a later request; a
nonmatching line is consumed as a decline, not saved as your next chat message.
Additional pretyped lines remain queued in the terminal.
The published v0.2.2 and older candidate still use `y/N` prompts.

**Unreleased source:** when session policy blocks all shell commands with a
universal rule, run, chat and the full-screen interface omit the shell tool and
explain that limitation. Other permitted work can still finish. Partial command
restrictions keep the tool available, with the same policy and approval checks.
Resuming after a policy change updates the model's capability notice while
preserving the recorded conversation. Shell availability is not proof that tests
ran or that a change is correct.

**Unreleased source:** tool progress reports observed file publication and shell
outcomes separately from the model's answer. A command exiting with code 0 means
that command reported success; it does not establish that it tested the right thing.
A check before a later edit does not validate the later file state. Denied, cancelled
and timed-out commands are not successful checks.

The source build labels normal timeline execution as `completed`, rather than
`pass`. Its final notice asks you to inspect and check the result. These progress
facts are not a persistent verification record; older receipts and resumed history
cannot establish which checks validated a final result. A model saying "fixed" is
still a claim to verify. No new checks run automatically.

## Find commands and review work

| Control | Purpose |
| --- | --- |
| F1 | Show keyboard help; unreleased source also closes Help on a second press |
| `/help` | Open the command palette |
| `/model` | Inspect and explicitly choose a model |
| `/profile` | Inspect execution profiles |
| `/why` | Explain routing without sending another model task |
| Ctrl+F or `/search` | Search the conversation |
| `/timeline` | Inspect recorded turns; this does not restore files |
| `/quit` | Exit |

Use `git status --short`, `git diff`, and your project's tests after an editing task.
The [practice walkthrough](TUTORIALS.md#2-make-one-small-change-and-inspect-it) shows
how to compare a small change. File changes are not automatically rolled back.

For narrow windows, widen the terminal if a command or path is hard to read. For
plain output, use `nh --ascii on chat --model <route-id>`; `NO_COLOR` disables color.
Screen-reader behavior has not been verified with a user study.
