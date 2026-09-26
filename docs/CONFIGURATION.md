# Model preferences and catalog upgrades

These features are in v0.3.0-rc.1 and later source builds, not the published v0.2.2 download. For first use, follow [guided setup](GETTING_STARTED.md).

For provider retirements and new routes, see [model updates](MODEL_UPDATES.md).

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
