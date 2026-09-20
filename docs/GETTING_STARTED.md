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

Your model choice applies to the chat opened by setup. Follow the displayed command
to start another session with that model; setup does not change global defaults.

## If setup stops

- If the project folder is wrong, cancel and open a terminal in the correct folder.
- If the catalog is untrusted, review it before following the trust instructions.
  This can also happen after an upgrade when a project still has an older bundled
  catalog. Setup preserves that file; it does not automatically trust or replace
  a repository's credential destinations. To use the new bundled catalog, keep
  a backup of the old file outside the project, then remove the project's
  `catalog.toml` and rerun setup. Do this only if you intend to replace its routes.
- If the credential store is unavailable, use `nh doctor` to inspect the setup.
  Do not save the key in a file as a workaround.
- Run setup in an interactive terminal. Piped answers cannot authorize setup.

For explicitly configured local models, see the [local models guide](LOCAL_MODELS.md).
