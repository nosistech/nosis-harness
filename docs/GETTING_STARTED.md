# Guided setup

Guided setup is available in v0.2.2 and later. For download verification and portable
installation, start with the [Windows quickstart](WINDOWS_QUICKSTART.md).

## Start in your project

Open a terminal in the folder you want Nosis to work on, then run:

```powershell
nh setup
```

Running `nh` without arguments opens the same guide. Use `nh --help` to list commands.

For a portable copy that is not on PATH, use the executable's full path:

```powershell
& 'C:\Users\you\Apps\Nosis\nh.exe' setup
```

Setup confirms the project folder before it creates configuration. It preserves
existing configuration and Git hooks; missing ignore entries may be added. Choose a
cloud model from the trusted catalog;
Nosis does not switch providers for you.

Press Enter at a yes/no question to decline. Type `cancel`, `q`, or `quit` to stop. Changes already
confirmed, such as creating project configuration, remain when you stop.

You can try a route and cost explanation without an API key. This preview makes
no model request and does not run the task. Prices are estimates from the catalog.

## Connect when you are ready

You need an API key from the provider you choose. A subscription to a provider's
chat app may not include API credit. Setup offers a hidden key prompt and stores
the key in the operating system credential store. Do not paste keys into task
messages, shell commands, or project files.

If a key is already stored for the selected route, setup keeps it and skips key entry.

Starting a chat is a separate choice. Cloud requests send task context to the
selected provider and may incur charges. Review each shell command before you
approve it. Setup does not change your approval policy.

In v0.2.2, your model choice applies to the chat opened by setup. Follow the displayed
command to start another session with that model.

## Remember your choice (v0.3.0-rc.1 candidate)

The current source build offers a separate, default-no question to remember the
selected model. Answering yes saves only the route ID in `~/.nosis/model`, where
`~` is your user home directory. The preference applies across projects. It does
not store a key, approve a destination or change permissions, budgets or profiles.

```powershell
nh model show
nh model set deepseek-v4-flash
nh model clear
```

For `run`, `chat`, `tui` and `profile`, explicit `--model` wins over the preference;
without either, the existing built-in default applies. Scripts that need a fixed
route should always pass `--model`. A saved route must exist in the currently
trusted catalog; an invalid or stale preference stops with recovery instructions.
Clearing it restores the built-in default. Resume retains the session's route;
`why` still treats `--model` as an optional comparison.

If a saved-model update is interrupted and recovery files remain, Nosis refuses to
use a default in place of the missing preference. Follow the recovery message and
choose explicitly with `nh model set <id>`. Preference updates have a brief
absent-file window and are not crash-atomic.
For an oversized file, symlink or other refused preference path, inspect
`~/.nosis/model` manually before repairing it; management commands will not blindly
replace an unsafe file. Explicit `--model <id>` remains available for a session.

## Upgrade your project catalog (v0.3.0-rc.1 candidate)

If an updated binary rejects an older bundled catalog, run `nh catalog migrate`
in an interactive terminal. Review the affected path, provider/model/capability/
price changes and credential destinations before answering yes. Enter declines.
Only recognized historical bundled catalogs are eligible. LF/CRLF line-ending
differences are accepted; other modifications or custom catalogs are refused and
preserved for manual review. The backup retains the original file's exact bytes.

Migration retains a versioned `catalog.toml.nh-backup-*` file beside the catalog.
An existing backup is never overwritten. Close other Nosis instances and editors
touching the catalog during migration. The source is rechecked before removal,
and replacement/rollback will not overwrite a newly created destination. There
is a brief absent-file window, and this is not crash-atomic or complete protection
against concurrent writers. If interrupted, inspect the backup and reported path
before restoring it; do not overwrite another process's new catalog.
If an attempt leaves a backup and then fails, resolve the reported cause and move
the reviewed backup to another safe name before retrying. Keep the backup contents.

The migration requires filesystem hard-link support and fails closed without it.
On a copied or portable FAT/exFAT project, use a supported filesystem or review and
replace the catalog manually with a retained backup. Never grant trust merely to
dismiss an upgrade error.

## If setup stops

- If the project folder is wrong, cancel and open a terminal in the correct folder.
- If the catalog is untrusted, review it before following the trust instructions.
  This can also happen after an upgrade when a project still has an older bundled
  catalog. Setup preserves that file; it does not automatically trust or replace
  a repository's credential destinations. In v0.2.2, to use the new bundled catalog, keep
  a backup of the old file outside the project, then remove the project's
  `catalog.toml` and rerun setup. Do this only if you intend to replace its routes.
- If the credential store is unavailable, use `nh doctor` to inspect the setup.
  Do not save the key in a file as a workaround.
- Run setup in an interactive terminal. Piped answers cannot authorize setup.

For explicitly configured local models, see the [local models guide](LOCAL_MODELS.md).
For complete practice tasks and expected results, see [three small tasks](TUTORIALS.md).
