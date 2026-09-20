//! `nh setup` - a guided, default-deny first run for one project folder.

use std::fs;
use std::io::{self, BufRead, IsTerminal as _, Write as _};
use std::path::Path;

use nh_core::terminal_capability::TerminalCapability;
use nh_routes::{RouteClass, RouteResolver};
use nh_vault::{KeyringVault, Scrubber};

use crate::{cmd_chat, cmd_doctor, cmd_init, cmd_key, cmd_run, cmd_why};

const MAX_PROMPT_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetupRoute {
    id: String,
    provider: String,
    model_id: String,
    vault_entry: String,
}

enum PromptInput {
    Line(String),
    Eof,
    TooLong,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Answer {
    Yes,
    No,
    Cancel,
}

trait SetupUi {
    fn line(&mut self, line: &str) -> io::Result<()>;
    fn prompt(&mut self, prompt: &str) -> io::Result<PromptInput>;
}

trait SetupActions {
    fn initialize(&mut self, root: &Path) -> anyhow::Result<Vec<String>>;
    fn doctor(&mut self) -> anyhow::Result<()>;
    fn routes(&mut self, root: &Path) -> anyhow::Result<Vec<SetupRoute>>;
    fn preview(&mut self, route: &str) -> anyhow::Result<()>;
    fn key_exists(&mut self, entry: &str) -> anyhow::Result<bool>;
    fn add_key(&mut self, entry: &str) -> anyhow::Result<()>;
    fn chat(&mut self, route: &str) -> anyhow::Result<()>;
}

struct ConsoleUi {
    scrubber: Scrubber,
}

impl ConsoleUi {
    fn new() -> Self {
        Self {
            scrubber: Scrubber::new(Vec::new()),
        }
    }

    fn safe(&self, text: &str) -> String {
        cmd_run::safe_line(&self.scrubber, text)
    }
}

impl SetupUi for ConsoleUi {
    fn line(&mut self, line: &str) -> io::Result<()> {
        println!("{}", self.safe(line));
        Ok(())
    }

    fn prompt(&mut self, prompt: &str) -> io::Result<PromptInput> {
        {
            let mut stdout = io::stdout();
            write!(stdout, "{}", self.safe(prompt))?;
            stdout.flush()?;
        }

        let stdin = io::stdin();
        read_prompt_input(&mut stdin.lock())
    }
}

struct SystemActions {
    terminal_capability: TerminalCapability,
    forced_ascii: Option<bool>,
}

impl SetupActions for SystemActions {
    fn initialize(&mut self, root: &Path) -> anyhow::Result<Vec<String>> {
        initialize_project(root)
    }

    fn doctor(&mut self) -> anyhow::Result<()> {
        cmd_doctor::run(self.terminal_capability, self.forced_ascii)
    }

    fn routes(&mut self, root: &Path) -> anyhow::Result<Vec<SetupRoute>> {
        let (catalog_root, catalog) = cmd_run::find_catalog(root)?;
        if catalog_root != root {
            anyhow::bail!(
                "setup found a catalog outside the confirmed project folder - run `nh init` in the intended folder"
            );
        }
        let resolver = RouteResolver::from_toml(&catalog)?;
        let mut routes = Vec::new();
        for id in resolver.available() {
            let route = resolver.resolve(&id)?;
            if route.class() == RouteClass::Api {
                routes.push(SetupRoute {
                    id,
                    provider: route.provider().to_owned(),
                    model_id: route.model_id().to_owned(),
                    vault_entry: route.vault_entry().to_owned(),
                });
            }
        }
        if routes.is_empty() {
            anyhow::bail!("trusted catalog has no API routes to select");
        }
        Ok(routes)
    }

    fn preview(&mut self, route: &str) -> anyhow::Result<()> {
        cmd_why::run(None, Some(route), self.terminal_capability)
    }

    fn key_exists(&mut self, entry: &str) -> anyhow::Result<bool> {
        KeyringVault.entry_exists(entry)
    }

    fn add_key(&mut self, entry: &str) -> anyhow::Result<()> {
        cmd_key::add(entry)
    }

    fn chat(&mut self, route: &str) -> anyhow::Result<()> {
        cmd_chat::run(route, "balanced", self.terminal_capability)
    }
}

fn initialize_project(root: &Path) -> anyhow::Result<Vec<String>> {
    preflight_init_paths(root)?;
    cmd_init::init_at(root)
}

fn preflight_init_paths(root: &Path) -> anyhow::Result<()> {
    let nosis = root.join(".nosis");
    let nosis_exists = inspect_setup_path(&nosis, true, ".nosis")?;
    inspect_setup_path(&root.join("catalog.toml"), false, "catalog.toml")?;
    if nosis_exists {
        inspect_setup_path(&nosis.join("law.toml"), false, ".nosis/law.toml")?;
        inspect_setup_path(&nosis.join(".gitignore"), false, ".nosis/.gitignore")?;
    }
    Ok(())
}

fn inspect_setup_path(path: &Path, directory: bool, label: &str) -> anyhow::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if !metadata.file_type().is_symlink()
                && if directory {
                    metadata.is_dir()
                } else {
                    metadata.is_file()
                } =>
        {
            Ok(true)
        }
        Ok(_) => {
            let expected = if directory {
                "directory"
            } else {
                "regular file"
            };
            anyhow::bail!(
                "refused setup: {label} must be a {expected}; symlinks and other file types are not accepted"
            )
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => anyhow::bail!("could not inspect {label} before setup ({error})"),
    }
}

pub fn run(
    terminal_capability: TerminalCapability,
    forced_ascii: Option<bool>,
) -> anyhow::Result<()> {
    require_interactive(io::stdin().is_terminal(), io::stdout().is_terminal())?;

    let project = std::env::current_dir()?;
    let executable = std::env::current_exe()?;
    let mut ui = ConsoleUi::new();
    let mut actions = SystemActions {
        terminal_capability,
        forced_ascii,
    };
    guide(&project, &executable, &mut ui, &mut actions)
}

fn require_interactive(stdin_terminal: bool, stdout_terminal: bool) -> anyhow::Result<()> {
    if !stdin_terminal || !stdout_terminal {
        anyhow::bail!(
            "guided setup requires interactive stdin and stdout - open a terminal and run this command again; run `nh --help` for non-interactive commands"
        );
    }
    Ok(())
}

fn guide(
    project: &Path,
    executable: &Path,
    ui: &mut dyn SetupUi,
    actions: &mut dyn SetupActions,
) -> anyhow::Result<()> {
    ui.line("Nosis Harness guided setup")?;
    ui.line(&format!("Project folder: {}", project.display()))?;
    ui.line("Existing project files, policy, and Git hooks are preserved.")?;
    match ask_yes_no(ui, "Set up this folder? [y/N] ")? {
        Answer::Yes => {}
        Answer::No | Answer::Cancel => {
            ui.line("Setup cancelled. No files changed.")?;
            return Ok(());
        }
    }

    for line in actions.initialize(project)? {
        ui.line(&line)?;
    }
    ui.line("")?;
    ui.line("Install check:")?;
    actions.doctor()?;

    let routes = actions.routes(project).map_err(|error| {
        anyhow::anyhow!(
            "could not list trusted routes: {error}. Existing catalog.toml was preserved; review it before trusting or replacing it"
        )
    })?;
    ui.line("")?;
    ui.line("Choose a model for this chat session:")?;
    for (index, route) in routes.iter().enumerate() {
        let detail = if route.id == route.model_id {
            route.provider.clone()
        } else {
            format!("{}; provider model {}", route.provider, route.model_id)
        };
        ui.line(&format!("  {}. {} ({detail})", index + 1, route.id))?;
    }
    let Some(selected) = select_route(ui, &routes)? else {
        ui.line("Setup stopped before model selection. No provider request was sent.")?;
        ui.line(&format!(
            "Run setup later: {}",
            command_line(executable, &["setup"])
        ))?;
        return Ok(());
    };

    ui.line(&format!("Selected model: {}", selected.id))?;
    ui.line("This choice is not saved as a default.")?;
    ui.line("This preview compares prices. Your selected model stays the same.")?;
    match ask_yes_no(
        ui,
        "Open the nh why preview now? (no key or provider call) [y/N] ",
    )? {
        Answer::Yes => actions.preview(&selected.id)?,
        Answer::No => {}
        Answer::Cancel => {
            stop_after_init(ui)?;
            return Ok(());
        }
    }

    ui.line("")?;
    ui.line(&format!(
        "Cloud use: prompts and tool results for this model go to {}; provider charges may apply.",
        selected.provider
    ))?;
    ui.line(&format!(
        "Nosis will use only model {}; it will not fall back automatically.",
        selected.id
    ))?;

    if !safe_vault_entry(&selected.vault_entry) {
        anyhow::bail!(
            "selected route has an unsafe vault entry name - review the trusted catalog before adding a key"
        );
    }
    let key_present = actions.key_exists(&selected.vault_entry).map_err(|error| {
        anyhow::anyhow!(
            "could not check the secure credential store ({error}) - inspect it with {}",
            command_line(executable, &["doctor"])
        )
    })?;
    if key_present {
        ui.line(&format!(
            "Existing secure credential entry {} was found and will not be replaced.",
            selected.vault_entry
        ))?;
    } else {
        match ask_yes_no(
            ui,
            &format!(
                "Store an API key for {} in the secure credential store now? (input hidden) [y/N] ",
                selected.vault_entry
            ),
        )? {
            Answer::Yes => actions.add_key(&selected.vault_entry)?,
            Answer::No => ui.line(&format!(
                "Add it later: {}",
                command_line(executable, &["key", "add", &selected.vault_entry])
            ))?,
            Answer::Cancel => {
                stop_after_init(ui)?;
                return Ok(());
            }
        }
    }

    match ask_yes_no(
        ui,
        &format!(
            "Start chat with {} now? Your messages may incur provider charges. [y/N] ",
            selected.id
        ),
    )? {
        Answer::Yes => actions.chat(&selected.id),
        Answer::No => {
            ui.line("Setup complete. No provider request was sent.")?;
            print_next_commands(ui, executable, selected)?;
            Ok(())
        }
        Answer::Cancel => {
            stop_after_init(ui)?;
            Ok(())
        }
    }
}

fn stop_after_init(ui: &mut dyn SetupUi) -> io::Result<()> {
    ui.line("Setup stopped. Existing setup changes were kept. No provider request was sent.")
}

fn print_next_commands(
    ui: &mut dyn SetupUi,
    executable: &Path,
    route: &SetupRoute,
) -> io::Result<()> {
    ui.line(&format!(
        "Compare prices: {}",
        command_line(executable, &["why", "--model", &route.id])
    ))?;
    ui.line(&format!(
        "Start chat later: {}",
        command_line(executable, &["chat", "--model", &route.id])
    ))
}

fn ask_yes_no(ui: &mut dyn SetupUi, prompt: &str) -> anyhow::Result<Answer> {
    loop {
        match ui.prompt(prompt)? {
            PromptInput::Eof | PromptInput::TooLong => return Ok(Answer::Cancel),
            PromptInput::Line(line) => {
                match line.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}') {
                    "y" | "Y" | "yes" | "Yes" | "YES" => return Ok(Answer::Yes),
                    "" | "n" | "N" | "no" | "No" | "NO" => return Ok(Answer::No),
                    "cancel" | "Cancel" | "q" | "quit" => return Ok(Answer::Cancel),
                    _ => ui.line("Enter y or n, or type cancel to stop.")?,
                }
            }
        }
    }
}

fn select_route<'a>(
    ui: &mut dyn SetupUi,
    routes: &'a [SetupRoute],
) -> anyhow::Result<Option<&'a SetupRoute>> {
    loop {
        match ui.prompt("Model number (blank or skip to stop): ")? {
            PromptInput::Eof | PromptInput::TooLong => return Ok(None),
            PromptInput::Line(line) => {
                let value = line.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
                if matches!(value, "" | "s" | "skip" | "cancel" | "q" | "quit") {
                    return Ok(None);
                }
                if let Ok(index) = value.parse::<usize>() {
                    if let Some(route) = index.checked_sub(1).and_then(|index| routes.get(index)) {
                        return Ok(Some(route));
                    }
                }
                ui.line(&format!(
                    "Enter a model number from 1 to {}, or skip.",
                    routes.len()
                ))?;
            }
        }
    }
}

fn read_prompt_input(reader: &mut dyn BufRead) -> io::Result<PromptInput> {
    let mut bytes = Vec::new();
    let mut saw_input = false;
    let mut too_long = false;

    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            if !saw_input {
                return Ok(PromptInput::Eof);
            }
            break;
        }
        saw_input = true;

        let newline = available.iter().position(|byte| *byte == b'\n');
        let content_len = newline.unwrap_or(available.len());
        if !too_long {
            let remaining = (MAX_PROMPT_BYTES + 1).saturating_sub(bytes.len());
            let copy_len = content_len.min(remaining);
            bytes.extend_from_slice(&available[..copy_len]);
            too_long = copy_len < content_len || bytes.len() > MAX_PROMPT_BYTES;
        }

        let consumed = newline.map_or(available.len(), |index| index + 1);
        let complete = newline.is_some();
        reader.consume(consumed);
        if complete {
            break;
        }
    }

    if too_long {
        return Ok(PromptInput::TooLong);
    }
    String::from_utf8(bytes)
        .map(PromptInput::Line)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "setup input is not valid UTF-8"))
}

fn safe_vault_entry(entry: &str) -> bool {
    !entry.is_empty()
        && entry.len() <= 128
        && entry
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn command_line(executable: &Path, arguments: &[&str]) -> String {
    let mut command = String::new();
    if cfg!(windows) {
        command.push_str("& ");
    }
    command.push_str(&shell_argument(&executable.to_string_lossy()));
    for argument in arguments {
        command.push(' ');
        command.push_str(&shell_argument(argument));
    }
    command
}

fn shell_argument(value: &str) -> String {
    if cfg!(windows) {
        format!("'{}'", value.replace('\'', "''"))
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::path::PathBuf;

    #[derive(Default)]
    struct TestUi {
        input: VecDeque<PromptInput>,
        output: String,
    }

    impl TestUi {
        fn with_lines(lines: &[&str]) -> Self {
            Self {
                input: lines
                    .iter()
                    .map(|line| PromptInput::Line((*line).to_owned()))
                    .collect(),
                output: String::new(),
            }
        }
    }

    impl SetupUi for TestUi {
        fn line(&mut self, line: &str) -> io::Result<()> {
            self.output.push_str(line);
            self.output.push('\n');
            Ok(())
        }

        fn prompt(&mut self, prompt: &str) -> io::Result<PromptInput> {
            self.output.push_str(prompt);
            Ok(self.input.pop_front().unwrap_or(PromptInput::Eof))
        }
    }

    struct TestActions {
        routes: Vec<SetupRoute>,
        key_present: bool,
        calls: Vec<String>,
    }

    impl TestActions {
        fn new(key_present: bool) -> Self {
            Self {
                routes: vec![
                    SetupRoute {
                        id: "first-route".to_owned(),
                        provider: "provider-one".to_owned(),
                        model_id: "provider-model".to_owned(),
                        vault_entry: "provider-one".to_owned(),
                    },
                    SetupRoute {
                        id: "second-route".to_owned(),
                        provider: "provider-two".to_owned(),
                        model_id: "second-route".to_owned(),
                        vault_entry: "provider-two".to_owned(),
                    },
                ],
                key_present,
                calls: Vec::new(),
            }
        }
    }

    impl SetupActions for TestActions {
        fn initialize(&mut self, _root: &Path) -> anyhow::Result<Vec<String>> {
            self.calls.push("init".to_owned());
            Ok(vec!["initialized".to_owned()])
        }

        fn doctor(&mut self) -> anyhow::Result<()> {
            self.calls.push("doctor".to_owned());
            Ok(())
        }

        fn routes(&mut self, _root: &Path) -> anyhow::Result<Vec<SetupRoute>> {
            self.calls.push("routes".to_owned());
            Ok(self.routes.clone())
        }

        fn preview(&mut self, route: &str) -> anyhow::Result<()> {
            self.calls.push(format!("preview:{route}"));
            Ok(())
        }

        fn key_exists(&mut self, entry: &str) -> anyhow::Result<bool> {
            self.calls.push(format!("key-exists:{entry}"));
            Ok(self.key_present)
        }

        fn add_key(&mut self, entry: &str) -> anyhow::Result<()> {
            self.calls.push(format!("add-key:{entry}"));
            Ok(())
        }

        fn chat(&mut self, route: &str) -> anyhow::Result<()> {
            self.calls.push(format!("chat:{route}"));
            Ok(())
        }
    }

    fn project() -> PathBuf {
        PathBuf::from("C:/work/project")
    }

    fn executable() -> PathBuf {
        PathBuf::from("C:/portable/Nosis Harness/nh.exe")
    }

    #[test]
    fn first_decline_has_no_side_effects() {
        let mut ui = TestUi::with_lines(&[""]);
        let mut actions = TestActions::new(false);

        guide(&project(), &executable(), &mut ui, &mut actions).unwrap();

        assert!(actions.calls.is_empty());
        assert!(ui.output.contains("No files changed"));
    }

    #[test]
    fn eof_before_confirmation_has_no_side_effects() {
        let mut ui = TestUi::default();
        let mut actions = TestActions::new(false);

        guide(&project(), &executable(), &mut ui, &mut actions).unwrap();

        assert!(actions.calls.is_empty());
        assert!(ui.output.contains("No files changed"));
    }

    #[test]
    fn selected_route_reaches_preview_and_chat_while_existing_key_is_preserved() {
        let mut ui = TestUi::with_lines(&["y", "2", "y", "y"]);
        let mut actions = TestActions::new(true);

        guide(&project(), &executable(), &mut ui, &mut actions).unwrap();

        assert_eq!(
            actions.calls,
            [
                "init",
                "doctor",
                "routes",
                "preview:second-route",
                "key-exists:provider-two",
                "chat:second-route",
            ]
        );
        assert!(!actions.calls.iter().any(|call| call.starts_with("add-key")));
        assert!(ui.output.contains("will not be replaced"));
        assert!(ui.output.contains("provider model provider-model"));
    }

    #[test]
    fn no_key_path_stays_offline_and_prints_portable_next_commands() {
        let mut ui = TestUi::with_lines(&["y", "1", "y", "n", "n"]);
        let mut actions = TestActions::new(false);

        guide(&project(), &executable(), &mut ui, &mut actions).unwrap();

        assert!(actions.calls.contains(&"preview:first-route".to_owned()));
        assert!(!actions.calls.iter().any(|call| call.starts_with("add-key")));
        assert!(!actions.calls.iter().any(|call| call.starts_with("chat")));
        assert!(ui.output.contains("No provider request was sent"));
        let expected = if cfg!(windows) {
            [
                "Add it later: & 'C:/portable/Nosis Harness/nh.exe' 'key' 'add' 'provider-one'",
                "Compare prices: & 'C:/portable/Nosis Harness/nh.exe' 'why' '--model' 'first-route'",
                "Start chat later: & 'C:/portable/Nosis Harness/nh.exe' 'chat' '--model' 'first-route'",
            ]
        } else {
            [
                "Add it later: 'C:/portable/Nosis Harness/nh.exe' 'key' 'add' 'provider-one'",
                "Compare prices: 'C:/portable/Nosis Harness/nh.exe' 'why' '--model' 'first-route'",
                "Start chat later: 'C:/portable/Nosis Harness/nh.exe' 'chat' '--model' 'first-route'",
            ]
        };
        for line in expected {
            assert!(ui.output.contains(line), "missing {line:?}: {}", ui.output);
        }
    }

    #[test]
    fn eof_at_optional_preview_stops_before_key_or_chat() {
        let mut ui = TestUi::with_lines(&["y", "1"]);
        let mut actions = TestActions::new(false);

        guide(&project(), &executable(), &mut ui, &mut actions).unwrap();

        assert_eq!(actions.calls, ["init", "doctor", "routes"]);
        assert!(ui.output.contains("Setup stopped"));
    }

    #[test]
    fn oversized_model_choice_stops_before_selection_actions() {
        let mut ui = TestUi::with_lines(&["y"]);
        ui.input.push_back(PromptInput::TooLong);
        let mut actions = TestActions::new(false);

        guide(&project(), &executable(), &mut ui, &mut actions).unwrap();

        assert_eq!(actions.calls, ["init", "doctor", "routes"]);
        assert!(ui.output.contains("Setup stopped before model selection"));
    }

    #[test]
    fn oversized_preview_answer_stops_before_key_or_chat() {
        let mut ui = TestUi::with_lines(&["y", "1"]);
        ui.input.push_back(PromptInput::TooLong);
        let mut actions = TestActions::new(false);

        guide(&project(), &executable(), &mut ui, &mut actions).unwrap();

        assert_eq!(actions.calls, ["init", "doctor", "routes"]);
        assert!(ui.output.contains("Setup stopped"));
    }

    #[test]
    fn eof_at_key_prompt_stops_before_key_or_chat() {
        let mut ui = TestUi::with_lines(&["y", "1", "n"]);
        let mut actions = TestActions::new(false);

        guide(&project(), &executable(), &mut ui, &mut actions).unwrap();

        assert!(actions
            .calls
            .contains(&"key-exists:provider-one".to_owned()));
        assert!(!actions.calls.iter().any(|call| call.starts_with("add-key")));
        assert!(!actions.calls.iter().any(|call| call.starts_with("chat")));
        assert!(ui.output.contains("Setup stopped"));
    }

    #[test]
    fn eof_at_chat_prompt_stops_without_starting_chat() {
        let mut ui = TestUi::with_lines(&["y", "1", "n"]);
        let mut actions = TestActions::new(true);

        guide(&project(), &executable(), &mut ui, &mut actions).unwrap();

        assert!(actions
            .calls
            .contains(&"key-exists:provider-one".to_owned()));
        assert!(!actions.calls.iter().any(|call| call.starts_with("chat")));
        assert!(ui.output.contains("Setup stopped"));
    }

    #[test]
    fn key_prompt_calls_existing_hidden_key_path_only_after_yes() {
        let mut ui = TestUi::with_lines(&["y", "1", "n", "y", "n"]);
        let mut actions = TestActions::new(false);

        guide(&project(), &executable(), &mut ui, &mut actions).unwrap();

        assert!(actions.calls.contains(&"add-key:provider-one".to_owned()));
        assert!(!actions.calls.iter().any(|call| call.starts_with("chat")));
    }

    #[test]
    fn interactive_gate_requires_both_input_and_output_terminals() {
        assert!(require_interactive(true, true).is_ok());
        let error = require_interactive(false, true).unwrap_err().to_string();
        assert!(error.contains("nh --help"));
        assert!(require_interactive(true, false).is_err());
        assert!(require_interactive(false, false).is_err());
    }

    #[test]
    fn prompt_reader_is_bounded_and_drains_the_line() {
        let mut input = io::Cursor::new(format!("{}\ny\n", "x".repeat(MAX_PROMPT_BYTES + 1)));
        assert!(matches!(
            read_prompt_input(&mut input).unwrap(),
            PromptInput::TooLong
        ));
        assert!(matches!(
            read_prompt_input(&mut input).unwrap(),
            PromptInput::Line(line) if line == "y"
        ));
    }

    #[test]
    fn unsafe_vault_entry_never_reaches_key_actions() {
        let mut ui = TestUi::with_lines(&["y", "1", "n"]);
        let mut actions = TestActions::new(false);
        actions.routes[0].vault_entry = "unsafe\nentry".to_owned();

        let error = guide(&project(), &executable(), &mut ui, &mut actions).unwrap_err();

        assert!(error.to_string().contains("unsafe vault entry"));
        assert!(!actions
            .calls
            .iter()
            .any(|call| call.starts_with("key-exists") || call.starts_with("add-key")));
    }

    #[test]
    fn shell_argument_quotes_apostrophes_for_the_active_shell() {
        let quoted = shell_argument("owner's model");
        if cfg!(windows) {
            assert_eq!(quoted, "'owner''s model'");
        } else {
            assert_eq!(quoted, "'owner'\"'\"'s model'");
        }
    }

    #[cfg(unix)]
    fn symlink_directory(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn symlink_directory(target: &Path, link: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    #[test]
    fn setup_preflight_refuses_nosis_symlink_without_touching_outside_files() {
        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let sentinel = outside.path().join("sentinel.txt");
        fs::write(&sentinel, "keep this exact value").unwrap();
        if let Err(error) = symlink_directory(outside.path(), &project.path().join(".nosis")) {
            if matches!(
                error.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
            ) {
                eprintln!("skipping setup symlink fixture: {error}");
                return;
            }
            panic!("could not create setup symlink fixture: {error}");
        }

        let error = initialize_project(project.path()).unwrap_err();

        assert!(error.to_string().contains(".nosis must be a directory"));
        assert_eq!(
            fs::read_to_string(&sentinel).unwrap(),
            "keep this exact value"
        );
        assert!(!project.path().join("catalog.toml").exists());
    }

    #[test]
    fn setup_route_listing_refuses_and_preserves_an_untrusted_catalog() {
        let project = tempfile::tempdir().unwrap();
        let catalog = project.path().join("catalog.toml");
        let original = "# operator catalog that has not been trusted\n";
        fs::write(&catalog, original).unwrap();
        let mut actions = SystemActions {
            terminal_capability: TerminalCapability::AsciiFallback,
            forced_ascii: Some(true),
        };

        let error = actions.routes(project.path()).unwrap_err();

        assert!(error.to_string().contains("not trusted"));
        assert_eq!(fs::read_to_string(catalog).unwrap(), original);
    }
}
