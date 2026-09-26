# Follow a task through the security boundaries

Start with [architecture](ARCHITECTURE_OVERVIEW.md) for component ownership and
[security limits](SECURITY_MODEL.md) for what the program does not isolate.
This map names the code to inspect; it does not replace reading the implementation.

| Question | Start here | What to verify |
| --- | --- | --- |
| Who can choose a destination? | [Resolver](../crates/nh-routes/src/resolver.rs), [catalog validation](../crates/nh-routes/src/resolver/catalog.rs) | Only the resolver constructs trusted routes; configuration cannot quietly redirect credentials |
| When is a key loaded? | [Credential boundary](../crates/nh-core/src/credential.rs), [vault](../crates/nh-vault/src/lib.rs) | Exact authorized origin checked before the active key is obtained; no redirect-based credential reuse |
| What can repository policy change? | [Policy loading](../crates/nh-law/src/load.rs), [matching](../crates/nh-law/src/matcher.rs) | Repository inputs can tighten restrictions; they cannot grant trust or automatic approval |
| Which policy do tools receive? | [Tool context](../crates/nh-tools/src/lib.rs), [run](../crates/nh-cli/src/cmd_run.rs), [chat startup](../crates/nh-cli/src/cmd_chat/startup.rs), [TUI worker](../crates/nh-tui/src/worker.rs), [Fleet engine](../crates/nh-fleet/src/engine.rs) | Every production context explicitly supplies its guard and session scrubber |
| What prevents shell execution? | [Shell execution](../crates/nh-tools/src/exec.rs) | Block stops execution; every other verdict still requires explicit approval; cancellation rechecked before launch |
| How are paths and file writes handled? | [File orchestration and containment](../crates/nh-tools/src/lib.rs), [publication helpers](../crates/nh-tools/src/file_publication.rs) | Containment, protected paths, no-clobber creation and conflict checks; understand the documented race limits |
| What does read-only expose? | [Tool registry and guard](../crates/nh-tools/src/lib.rs), [run](../crates/nh-cli/src/cmd_run.rs) | Exactly the guarded read/search tools; writes, exec and sends blocked; receipts and provider calls still occur |
| Can tool text authorize another action? | [Agent loop](../crates/nh-core/src/agent.rs), [MCP adapter](../crates/nh-tools/src/mcp/adapter.rs) | Output is untrusted data; actual actions go through tool policy/approval again |
| Does MCP discovery bypass approval? | [Discovery and invocation](../crates/nh-tools/src/mcp/adapter.rs), [bounded registry](../crates/nh-tools/src/mcp/client.rs) | Only returned available names may invoke their original adapter; duplicates refused, limits enforced, original trust/send/approval checks retained |
| What can leave through output? | [Scrubber](../crates/nh-vault/src/lib.rs), [CLI output](../crates/nh-cli/src/cmd_run.rs), [tool rendering](../crates/nh-tools/src/lib.rs) | Active credentials and known secret shapes redacted; terminal control characters handled |
| What does a receipt prove? | [Receipt](../crates/nh-core/src/receipt.rs), [agent loop](../crates/nh-core/src/agent.rs) | Execution outcome and reported usage; normal completion alone does not prove task correctness |
| What do efficiency records contain? | [Opt-in recorder](../crates/nh-core/src/efficiency.rs), [task report](../scripts/efficiency-report.py) | Metadata only; unknown billing stays unknown; every attempt belongs to a separately judged trial |
| Can a range read bypass file policy? | [Ranged reader](../crates/nh-tools/src/ranged_read.rs) | Same containment, guard, approval and scrubber as ordinary reads; bounded scan and cancellation |
| Who can retrieve retained observations? | [Session storage and reader](../crates/nh-tools/src/observation.rs) | Scrubbed text, exact session/boundary handles, digest verification, quotas and narrow cleanup; Windows inherits directory permissions |
| What can experimental compaction remove? | [Context experiment](../crates/nh-core/src/context_experiment.rs) | Only complete archived assistant/tool groups; original system/user messages and recent pairs retained; reasoning preserved; archive failure keeps unarchived history |
| What survives cancellation/resume? | [Session ledger](../crates/nh-core/src/session_ledger.rs), [TUI worker](../crates/nh-tui/src/worker.rs) | New actions stop after observed cancellation; completed changes persist; unknown usage stays unknown |
| How does Fleet execute tools? | [Fleet engine](../crates/nh-fleet/src/engine.rs), [preparation](../crates/nh-fleet/src/prepare.rs) | Trace installed policy, approval handling and scrubbed output through the preview execution path |
| What can call the inbound MCP server? | [Server](../crates/nh-mcp/src/lib.rs), [requests](../crates/nh-mcp/src/request.rs), [Fleet bridge](../crates/nh-mcp/src/fleet_tools.rs) | Loopback binding, bearer authentication and bounded dispatch; inbound access does not authorize arbitrary shell execution |

Tests beside these modules exercise refusals as well as allowed operations. Begin
with a negative case, then trace the production caller that installs the tested
policy. A passing helper test is insufficient if a caller can omit the helper.

Run the documented checks in [CONTRIBUTING.md](../CONTRIBUTING.md). Keep mechanical
module movement separate from behavior changes. Review third-party dependencies
as part of the supply chain; a workspace Rust lint is not a guarantee about all
dependency internals.
