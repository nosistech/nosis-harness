//! Constitution assembly and immutable session policy for Nosis Harness.
//!
//! Policy is data: bundled, user-global, and repository law files are merged once
//! at session start. Repository law may add protections, never weaken them.

mod load;
mod matcher;
mod model;

pub use load::{
    assemble_constitution, load, load_checked, read_guarded, user_home_dir, GuardedRead,
};

/// Segment-wise iterative glob matching: no recursion, so adversarial patterns
/// cannot exhaust the stack, and `**` spans directory segments.
pub use matcher::glob_matches;
pub use model::{Autonomy, ConstitutionSources, Law, LoadOptions, Policy, PolicyView, Verdict};

#[cfg(test)]
use load::{
    compile_policy, load_checked_with_home, load_with_home, parse_law,
    read_guarded_with_before_open,
};

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
