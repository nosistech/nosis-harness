# Why these features

Research cutoff: September 26, 2026; implementation checks continued September 27.
Unversioned product documentation reflects the time it was read. These are reasons for the current
product direction, not claims that Nosis has beaten another assistant.

**Start with a useful, checkable task.** A short text exercise removes the need to
install programming tools just to learn the interface. It also separates a free
local estimate from a model request that can incur charges. The first-task folder
is bundled by rc.3 source packaging; it is absent from older Windows downloads.

**Show observed changes and command outcomes.** An assistant saying it finished
is not independent verification. Users need to inspect the actual work and decide
whether the right checks ran. A position paper on
[human-centered coding agents](https://arxiv.org/abs/2608.12355) motivates this
focus; it does not prove that our interface is easy to use.

**Preserve work during recovery.** Recent firsthand reports describe
[missing resumed answers](https://github.com/anthropics/claude-code/issues/96812)
and [interrupted waiting sessions](https://github.com/anomalyco/opencode/issues/51343)
in other products. Their architectures differ from Nosis. We use these as prompts
for regression tests, not as evidence of a competitive advantage.

**Keep control explicit.** A review screen is not rollback. Stopping a task cannot
undo completed commands. Provider selection remains a user's decision. Research
on [context privilege escalation](https://arxiv.org/abs/2609.01222) also supports
keeping external data separate from trusted instructions and enforced permissions.
Nosis has no operating-system sandbox; see [security boundaries](SECURITY_MODEL.md).

**Test the whole task.** [HarnessDev](https://arxiv.org/abs/2609.01437) finds that
iterative harness improvements do not reliably transfer to unseen tasks. The
[Scaffold Effect study](https://arxiv.org/abs/2607.22585) shows why the model and
harness must be evaluated together. We need success, elapsed time, correction
effort and complete cost evidence, not just a smaller prompt or a happy-path demo.

Our [newcomer protocol](USABILITY_CHECK.md) records actual assistance and failures.
Automated rendering and regression tests cannot replace those observations.
The design deliberately avoids a new desktop framework, hidden paid retries and
automatic provider changes. Those would add complexity or change the user's
control without evidence that they improve the first experience.
