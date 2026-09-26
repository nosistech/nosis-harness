# Command reference

Generated from the current source build. Unreleased commands may not exist in the
latest downloadable package. Run `nh --help` to check your installed version.
For a learning path, start with [three small tasks](TUTORIALS.md).
Reasoning defaults vary by route and can be `max`. On GLM 5.3 routes,
`--think none` becomes `low`; reasoning stays enabled and can incur charges.
See [model updates](MODEL_UPDATES.md) before selecting those routes.

## nh

```text
Nosis Harness - multi-model terminal agent

Usage: nh.exe [OPTIONS] <COMMAND>

Commands:
  setup    Set up this project, choose a model, and optionally start chat
  catalog  Review and migrate a known historical bundled catalog
  init     Set up .nosis/ in this repo (receipts dir, .gitignore, secret-pattern pre-commit hook)
  key      Manage API keys in the OS-native vault (never echoed, never stored in files)
  model    Manage the operator-owned default model used when --model is omitted
  run      Run an agent task
  chat     Chat with a model - /model and /provider switch routes mid-session
  doctor   Check the install and print what is wrong and how to fix it
  resume   Resume an interrupted chat or TUI session
  why      Explain the cheapest capable route for a task estimate
  profile  List execution profiles and their caps for a model
  tui      Open the full-screen terminal UI
  fleet    Run independent tasks in a durable worker fleet
  mcp      Serve the local MCP endpoint (preview; 127.0.0.1 only)
  help     Print this message or the help of the given subcommand(s)

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
  -V, --version        Print version
```

## nh setup

```text
Set up this project, choose a model, and optionally start chat

Usage: nh.exe setup [OPTIONS]

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh init

```text
Set up .nosis/ in this repo (receipts dir, .gitignore, secret-pattern pre-commit hook)

Usage: nh.exe init [OPTIONS]

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh catalog

```text
Review and migrate a known historical bundled catalog

Usage: nh.exe catalog [OPTIONS] <COMMAND>

Commands:
  migrate  Replace an exact historical bundled catalog after an explicit review
  help     Print this message or the help of the given subcommand(s)

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh catalog migrate

```text
Replace an exact historical bundled catalog after an explicit review

Usage: nh.exe catalog migrate [OPTIONS]

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh model

```text
Manage the operator-owned default model used when --model is omitted

Usage: nh.exe model [OPTIONS] <COMMAND>

Commands:
  set    Save a default model after validating it against the trusted catalog
  show   Show and validate the saved default model
  clear  Remove the saved default model
  help   Print this message or the help of the given subcommand(s)

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh model set

```text
Save a default model after validating it against the trusted catalog

Usage: nh.exe model set [OPTIONS] <MODEL>

Arguments:
  <MODEL>

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh model show

```text
Show and validate the saved default model

Usage: nh.exe model show [OPTIONS]

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh model clear

```text
Remove the saved default model

Usage: nh.exe model clear [OPTIONS]

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh key

```text
Manage API keys in the OS-native vault (never echoed, never stored in files)

Usage: nh.exe key [OPTIONS] <COMMAND>

Commands:
  add     Prompt for a key and store it (e.g. `nh key add deepseek`)
  remove  Remove a key from the OS-native store (e.g. `nh key remove deepseek`)
  help    Print this message or the help of the given subcommand(s)

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh key add

```text
Prompt for a key and store it (e.g. `nh key add deepseek`)

Usage: nh.exe key add [OPTIONS] <ENTRY>

Arguments:
  <ENTRY>

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh key remove

```text
Remove a key from the OS-native store (e.g. `nh key remove deepseek`)

Usage: nh.exe key remove [OPTIONS] <ENTRY>

Arguments:
  <ENTRY>

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh run

```text
Run an agent task

Usage: nh.exe run [OPTIONS] <TASK>

Arguments:
  <TASK>  The task, in plain words

Options:
      --ascii <ASCII>
          Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
      --model <MODEL>
          Model id override; otherwise use the saved model or bundled default
      --max-turns <MAX_TURNS>
          Max agent turns before giving up with a timeout receipt [default: 20]
      --think <THINK>
          Thinking effort (default picked per route dialect) [possible values: none, low, high, max]
      --autonomy <AUTONOMY>
          Session autonomy override (default comes from law files) [possible values: ask, auto]
      --profile <PROFILE>
          Execution profile: frugal, balanced, or max-quality [default: balanced]
      --image <PATH>
          Attach a PNG or JPEG image (repeatable; maximum 4)
      --read-only
          Expose only guarded read tools; provider costs and local receipts still apply
      --measure-efficiency
          Append local metadata-only efficiency records under .nosis (preview)
      --enable-ranged-reads
          Let read_file accept bounded start_line/line_count arguments (preview)
      --retain-observations
          Retain scrubbed large tool results for bounded retrieval during this run (preview)
      --context-experiment <CONTEXT_EXPERIMENT>
          Experimental extractive context mode; requires retained observations [possible values: extractive-v1]
      --identity-prompt <IDENTITY_PROMPT>
          Versioned shorter identity clause; all safety and project law remain [possible values: compact-v1]
  -h, --help
          Print help
```

## nh chat

```text
Chat with a model - /model and /provider switch routes mid-session

Usage: nh.exe chat [OPTIONS]

Options:
      --ascii <ASCII>      Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
      --model <MODEL>      Model id override; otherwise use the saved model or bundled default
      --profile <PROFILE>  Execution profile: frugal, balanced, or max-quality [default: balanced]
      --mcp-discovery      Replace eager MCP schemas with fixed discovery and invocation tools (preview)
  -h, --help               Print help
```

## nh doctor

```text
Check the install and print what is wrong and how to fix it

Usage: nh.exe doctor [OPTIONS]

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh resume

```text
Resume an interrupted chat or TUI session

Usage: nh.exe resume [OPTIONS] [SESSION_ID]

Arguments:
  [SESSION_ID]  Session id; omit it to list interrupted sessions

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh why

```text
Explain the cheapest capable route for a task estimate

Usage: nh.exe why [OPTIONS] [TASK]

Arguments:
  [TASK]  Optional task text used for a rough token estimate

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
      --model <MODEL>  Explicitly selected model to compare with the cheapest capable route
  -h, --help           Print help
```

## nh profile

```text
List execution profiles and their caps for a model

Usage: nh.exe profile [OPTIONS]

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
      --model <MODEL>  Model id override; otherwise use the saved model or bundled default
  -h, --help           Print help
```

## nh tui

```text
Open the full-screen terminal UI

Usage: nh.exe tui [OPTIONS]

Options:
      --ascii <ASCII>      Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
      --model <MODEL>      Model id override; otherwise use the saved model or bundled default
      --budget <BUDGET>    Observed session token stop; the active turn is allowed to finish
      --profile <PROFILE>  Execution profile: frugal, balanced, or max-quality [default: balanced]
  -h, --help               Print help
```

## nh fleet

```text
Run independent tasks in a durable worker fleet

Usage: nh.exe fleet [OPTIONS] <COMMAND>

Commands:
  run     Start a new fleet from a JSON task file
  resume  Resume the latest incomplete run, or a specific run id
  help    Print this message or the help of the given subcommand(s)

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh fleet run

```text
Start a new fleet from a JSON task file

Usage: nh.exe fleet run [OPTIONS] <TASKS>

Arguments:
  <TASKS>

Options:
      --ascii <ASCII>
          Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
      --max-workers <MAX_WORKERS>

      --budget <BUDGET>
          Required observed-token dispatch budget (or set budget_tokens in the task file)
      --escalate [<ESCALATE>]
          [possible values: true, false]
      --defer-offpeak [<DEFER_OFFPEAK>]
          [possible values: true, false]
  -h, --help
          Print help
```

## nh fleet resume

```text
Resume the latest incomplete run, or a specific run id

Usage: nh.exe fleet resume [OPTIONS] [RUN_ID]

Arguments:
  [RUN_ID]

Options:
      --ascii <ASCII>              Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
      --max-workers <MAX_WORKERS>
  -h, --help                       Print help
```

## nh mcp

```text
Serve the local MCP endpoint (preview; 127.0.0.1 only)

Usage: nh.exe mcp [OPTIONS] <COMMAND>

Commands:
  serve  Start the local MCP server (route_resolve, fleet_run, fleet_status, why, route_cost, receipts)
  help   Print this message or the help of the given subcommand(s)

Options:
      --ascii <ASCII>  Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
  -h, --help           Print help
```

## nh mcp serve

```text
Start the local MCP server (route_resolve, fleet_run, fleet_status, why, route_cost, receipts)

Usage: nh.exe mcp serve [OPTIONS]

Options:
      --addr <ADDR>                [default: 127.0.0.1:8765]
      --ascii <ASCII>              Force one-column ASCII fallback glyphs on or off (default: decide from stdout) [possible values: on, off]
      --token-entry <TOKEN_ENTRY>
  -h, --help                       Print help
```
