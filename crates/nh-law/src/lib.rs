//! Constitution assembly and immutable session policy for Nosis Harness.
//!
//! Policy is data: bundled, user-global, and repository law files are merged once
//! at session start. Repository law may add protections, never weaken them.

mod load;
mod matcher;
mod model;

pub use load::{assemble_constitution, load, user_home_dir};

pub use model::{Autonomy, ConstitutionSources, Law, LoadOptions, Policy, PolicyView, Verdict};

#[cfg(test)]
use load::{compile_policy, load_with_home, parse_law};
#[cfg(test)]
use matcher::glob_matches;

const SECTION_JOINER: &str = "\n\n";
const OPERATING_LAW_LABEL: &str = "## Operating law";
const USER_LAW_LABEL: &str = "## User law";
const PROJECT_LAW_LABEL: &str = "## Project law";
const AGENTS_LABEL: &str = "## Project instructions (AGENTS.md)";
const MEMORY_LABEL: &str = "## Memory";
const MAX_CONSTITUTION_BYTES: usize = 64 * 1024;
const REPO_RESTRICTION_WARNING: &str =
    "repo .nosis/law.toml cannot raise autonomy, auto-approve paths, or approve credential audiences - ignored";

/// The bundled default constitution and policy.
pub const BUNDLED_LAW: &str = include_str!("bundled_law.toml");

/// Safe project-law starter written by `nh init`.
pub const STARTER_LAW_TOML: &str = include_str!("starter_law.toml");

#[cfg(test)]
mod tests;
