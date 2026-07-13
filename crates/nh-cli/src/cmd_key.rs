//! `nh key add` — hidden prompt, store in the OS-native vault, never echo the value.

use nh_vault::{KeyringVault, Vault};
use zeroize::Zeroizing;

#[cfg(target_os = "windows")]
const STORE: &str = "Windows Credential Manager";
#[cfg(target_os = "macos")]
const STORE: &str = "macOS Keychain";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const STORE: &str = "the OS keyring";

pub fn add(entry: &str) -> anyhow::Result<()> {
    let key = Zeroizing::new(
        rpassword::prompt_password(format!("key for {entry} (input hidden): ")).map_err(
            |_| anyhow::anyhow!("couldn't read input — run `nh key add {entry}` in an interactive terminal"),
        )?,
    );
    let value = key.trim();
    if value.is_empty() {
        anyhow::bail!("nothing entered — key not stored (run `nh key add {entry}` to try again)");
    }
    KeyringVault.set(entry, value)?;
    println!("stored {entry} in {STORE}");
    Ok(())
}
