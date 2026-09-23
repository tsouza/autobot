//! Reusable library behind the AutoBot development scripts.
//!
//! Every `scripts/<name>.rs` entry point is a thin rust-script that depends on this crate
//! by path and calls one function; the logic lives here, where it is linted and tested.

pub mod artifact_store;
pub mod banner;
pub mod cargo;
pub mod conventional;
pub mod design;
#[cfg(feature = "doctor")]
pub mod doctor;
pub mod formal;
pub mod gates;
pub mod git;
#[cfg(feature = "github")]
pub mod github;
#[cfg(feature = "hooks")]
pub mod hooks;
pub mod image;
#[cfg(feature = "judge")]
pub mod judge;
pub mod kind;
pub mod layering;
pub mod markdown;
pub mod process;
pub mod worktree;

/// Error type shared by every module.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A spawned command could not start or exited unsuccessfully.
    Command {
        /// The command line, for display.
        command: String,
        /// Exit status and captured standard error.
        detail: String,
    },
    /// Output that should have been JSON or UTF-8 was not.
    Parse(String),
    /// A GitHub API request failed.
    #[cfg(feature = "github")]
    Http(String),
    /// No GitHub API token could be found.
    #[cfg(feature = "github")]
    Token(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command { command, detail } => write!(f, "`{command}` failed: {detail}"),
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
            #[cfg(feature = "github")]
            Self::Http(msg) => write!(f, "GitHub API error: {msg}"),
            #[cfg(feature = "github")]
            Self::Token(msg) => write!(f, "no GitHub token: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

/// Result alias used by every module.
pub type Result<T, E = Error> = std::result::Result<T, E>;
