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

## Find commands and review work

| Control | Purpose |
| --- | --- |
| F1 | Show keyboard help |
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
