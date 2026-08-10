//! Process-edge terminal capability detection and display-glyph selection.

use std::borrow::Cow;
use std::io::IsTerminal as _;
use std::process::Command;

/// Result of probing the active Windows console code page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodePageStatus {
    Utf8,
    Other(u32),
    Unknown,
}

/// Display mode selected once at the process edge and passed to renderers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalCapability {
    Unicode,
    AsciiFallback,
}

impl TerminalCapability {
    /// Resolve the display mode from injected facts.
    pub const fn from_inputs(
        forced_ascii: Option<bool>,
        stdout_terminal: bool,
        windows: bool,
    ) -> Self {
        match (forced_ascii, stdout_terminal, windows) {
            (Some(true), _, _) => Self::AsciiFallback,
            (Some(false), _, _) => Self::Unicode,
            (None, false, _) => Self::AsciiFallback,
            (None, true, _) => Self::Unicode,
        }
    }

    /// Gather process facts once and resolve the display mode.
    pub fn from_process(forced_ascii: Option<bool>) -> Self {
        Self::from_inputs(forced_ascii, std::io::stdout().is_terminal(), cfg!(windows))
    }

    pub const fn uses_ascii_fallback(self) -> bool {
        matches!(self, Self::AsciiFallback)
    }

    /// Replace the approved one-column interface glyphs for the fallback mode.
    pub fn render_text<'a>(self, text: &'a str) -> Cow<'a, str> {
        if self == Self::Unicode {
            return Cow::Borrowed(text);
        }

        let mut rendered = String::with_capacity(text.len());
        for character in text.chars() {
            rendered.push(match character {
                '\u{00b7}' | '\u{2500}' => '-',
                '\u{25cf}' => '!',
                '\u{25cb}' => 'o',
                '\u{25c6}' => '*',
                '\u{276f}' | '\u{2192}' => '>',
                '\u{2191}' => '^',
                '\u{2193}' => 'v',
                '\u{2190}' => '<',
                '\u{00a5}' => 'Y',
                '\u{2248}' => '~',
                other => other,
            });
        }
        Cow::Owned(rendered)
    }
}

/// Probe the active Windows console code page, or return unknown elsewhere.
pub fn probe_code_page() -> CodePageStatus {
    if cfg!(windows) {
        probe_windows_code_page()
    } else {
        CodePageStatus::Unknown
    }
}

fn probe_windows_code_page() -> CodePageStatus {
    let output = match Command::new("chcp.com").output() {
        Ok(output) if output.status.success() => output,
        _ => return CodePageStatus::Unknown,
    };
    match trailing_ascii_integer(&output.stdout) {
        Some(65001) => CodePageStatus::Utf8,
        Some(value) => CodePageStatus::Other(value),
        None => CodePageStatus::Unknown,
    }
}

/// Parse the final ASCII integer from console command output.
pub fn trailing_ascii_integer(bytes: &[u8]) -> Option<u32> {
    let end = bytes.iter().rposition(u8::is_ascii_digit)? + 1;
    let start = bytes[..end]
        .iter()
        .rposition(|byte| !byte.is_ascii_digit())
        .map_or(0, |index| index + 1);
    std::str::from_utf8(&bytes[start..end]).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_order_uses_only_injected_facts() {
        assert_eq!(
            TerminalCapability::from_inputs(Some(true), true, false),
            TerminalCapability::AsciiFallback
        );
        assert_eq!(
            TerminalCapability::from_inputs(Some(false), false, false),
            TerminalCapability::Unicode
        );
        assert_eq!(
            TerminalCapability::from_inputs(None, false, false),
            TerminalCapability::AsciiFallback
        );
        assert_eq!(
            TerminalCapability::from_inputs(None, true, false),
            TerminalCapability::Unicode
        );
        assert_eq!(
            TerminalCapability::from_inputs(None, true, true),
            TerminalCapability::Unicode
        );
    }

    #[test]
    fn fallback_maps_named_glyphs_and_leaves_the_ellipsis() {
        let source = concat!(
            "\u{00b7}", "\u{2500}", "\u{25cf}", "\u{25cb}", "\u{25c6}", "\u{276f}", "\u{2191}",
            "\u{2193}", "\u{2192}", "\u{2190}", "\u{00a5}", "\u{2248}", "\u{2026}"
        );

        assert_eq!(
            TerminalCapability::AsciiFallback.render_text(source),
            "--!o*>^v><Y~\u{2026}"
        );
    }

    #[test]
    fn unicode_mode_keeps_every_named_glyph_unchanged() {
        let source = concat!(
            "\u{00b7}", "\u{2500}", "\u{25cf}", "\u{25cb}", "\u{25c6}", "\u{276f}", "\u{2191}",
            "\u{2193}", "\u{2192}", "\u{2190}", "\u{00a5}", "\u{2248}", "\u{2026}"
        );

        assert_eq!(TerminalCapability::Unicode.render_text(source), source);
    }

    #[test]
    fn parser_uses_the_trailing_ascii_integer() {
        assert_eq!(
            trailing_ascii_integer(b"Active code page: 437\r\n"),
            Some(437)
        );
        assert_eq!(
            trailing_ascii_integer(b"Active code page: 65001\r\n"),
            Some(65001)
        );
        assert_eq!(trailing_ascii_integer(b"code page unknown\r\n"), None);
    }
}
