# Check whether newcomers can use Nosis

This is a small usability protocol, not an adoption claim. Ask five willing people
who have not used Nosis, including people with little terminal experience. Give them
the tested release and its beginner guide. Do not send invitations automatically.
Use harmless practice folders; do not request private work, API keys or recordings
of password entry. Obtain consent before taking notes or recording a session.

## Tasks

1. Install, verify and open Nosis using the guide. Explain any Windows warning in
   their own words. Never ask them to disable antivirus or accept a detection.
2. Reach a no-key preview. Ask what it did and whether it contacted an AI provider.
3. With their own configured access if they choose, ask about a practice project.
   Explain what information can leave the computer and where the answer came from.
4. Make one small practice edit. Ask how they know it is correct and whether an
   assistant saying "done" is enough. Inspect the change and the provided checks.
5. Present a harmless shell approval request. Ask what approving permits and how to
   decline. Do not use destructive commands to test a person's vigilance.
6. Stop, return later and recover from one ordinary setup issue using the guide.

For the full-screen interface, also observe these everyday interactions. Resize
the window to 80 columns by 24 rows and then 60 by 15; ask the person to find Help,
identify the project and return to their draft. Paste two harmless lines and check
that nothing is sent until they choose to send. During a mock or harmless task,
ask whether Nosis is working, waiting for approval or stopping. Have them decline
an approval, find an older answer, and return to the latest message. Record lost
drafts, hidden controls and uncertainty, even if they eventually finish.
During approval, ask them to inspect Help and return without cancelling, then
read a harmless request longer than one screen before choosing. Ask what the
repeat-approval option permits; do not explain it until their answer is recorded.

For the unreleased full-screen build, also check an approval arriving while the
person is drafting the next message. Use harmless text such as `many tiny changes`
and a harmless pending request. Keep typing and paste another line, then press
Enter. The draft should remain queued, and the request should still await a choice.
Ask the person to find approve-once, repeat and decline from the visible hints,
without teaching the keys first. Observe whether their laptop requires Fn to use
F2/F3/F4. Do not count a facilitator pressing a key as unassisted completion.
F1 must open and close Help without approving; F4 must decline without discarding
the next-message draft. A dedicated key pressed while Help is open must not approve
the hidden request. These observations are pending, not implied by unit tests.

For plain chat on the current source, observe the fresh-code approval prompt too.
Ask the participant to decline with Enter, then approve one harmless request using
its displayed code. Record mistakes, difficulty reading/copying the code, and
whether they understand that `y/yes` no longer approves. A line typed before a
prompt is consumed as a decline; record any confusion about the next-message draft.

Check both Windows Terminal and the terminal the person normally uses. A terminal
can assign its own copy/paste shortcuts. If a participant uses a screen reader,
observe plain chat and the full-screen interface separately; do not infer support
from ASCII mode or automated rendering tests. Use larger text when requested.

If model access is unavailable, complete the no-key tasks and mark connected tasks
not tested. A facilitator explaining the intended answer counts as assistance.

## Record observations

Before each session, record the exact executable version and SHA256, operating
system, terminal, window size and whether the build is published or local. The
current source may report the same candidate version as an older release asset;
the hash distinguishes them. Do not record credentials or private project paths.
In PowerShell, obtain the hash with `Get-FileHash -Algorithm SHA256 <path-to-nh.exe>`.

| Build SHA256 | Version | OS / terminal | Window size | Date | Consent recorded |
| --- | --- | --- | --- | --- | --- |
| Not tested | | | | | |

| Participant alias | Terminal experience | Install time | First useful result time | Help needed | Permission misunderstanding | Completed tasks |
| --- | --- | --- | --- | --- | --- | --- |
| P1 | | | | | | |
| P2 | | | | | | |
| P3 | | | | | | |
| P4 | | | | | | |
| P5 | | | | | | |

Use aliases and avoid identifying details. Mark missing times unknown. Note the
exact point of confusion and the instruction or UI text encountered. Preserve
unsuccessful attempts rather than restarting until only a successful run remains.

Initial design target: four of five complete the first useful task without live
coaching and nobody is observed misunderstanding an approval. A five-person sample
cannot establish a population success rate or certify safe behavior. Fix the most
common or consequential obstacle, then check whether the change helped.

## Compare useful work fairly

Use the [practice checks](../examples/practice-tasks/README.md) as a warm-up, then
choose a small fixed set of real tasks with explicit expected results. Compare
project explanation, bug fixing and a bounded feature change. Inspect resulting
code before executing it. Use the same model, settings, starting files and task
text where possible; record differences when products do not support that match.

Record elapsed time, acceptance-check results, correction prompts, unexpected edits,
reported usage and whether usage is complete. Retain failed runs and rate limits.
Do not call missing cost zero. Do not publish "cost per correct task" for a run
whose cost is unknown, or a win rate based only on tasks chosen after seeing results.
Agree a spending limit before paid comparisons; this document runs no provider calls.

The existing free-model practice smoke is a development observation, not a newcomer
study or competitor comparison. Results remain pending until these tasks are observed.
