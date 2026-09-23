//! Running external commands with logging.

use crate::{Error, Result};
use std::path::Path;
use std::process::{Command, Stdio};

/// A command to run, built up before execution.
#[derive(Debug, Clone)]
pub struct Cmd {
    program: String,
    args: Vec<String>,
    dir: Option<std::path::PathBuf>,
}

impl Cmd {
    /// Starts a command for `program`.
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            dir: None,
        }
    }

    /// Appends arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Runs the command in `dir` instead of the current directory.
    #[must_use]
    pub fn current_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.dir = Some(dir.as_ref().to_path_buf());
        self
    }

    /// The command line as displayed in logs and errors.
    #[must_use]
    pub fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args);
        if let Some(dir) = &self.dir {
            cmd.current_dir(dir);
        }
        cmd
    }

    /// Runs the command with inherited standard streams, logging it to standard error first.
    ///
    /// # Errors
    /// Fails if the command cannot start or exits unsuccessfully.
    pub fn run(&self) -> Result<()> {
        self.run_logged(&mut std::io::stderr())
    }

    /// Runs the command like [`Cmd::run`], writing the `+ <command>` log line to `log`.
    ///
    /// # Errors
    /// Fails if the command cannot start or exits unsuccessfully.
    pub fn run_logged(&self, log: &mut impl std::io::Write) -> Result<()> {
        // A lost log line is not a reason to skip the command.
        let _ = writeln!(log, "+ {}", self.display());
        let status = self
            .command()
            .status()
            .map_err(|e| self.error(e.to_string()))?;
        if status.success() {
            Ok(())
        } else {
            Err(self.error(status.to_string()))
        }
    }

    /// Runs the command and returns its standard output as UTF-8.
    ///
    /// # Errors
    /// Fails if the command cannot start, exits unsuccessfully, or prints non-UTF-8 output.
    pub fn output(&self) -> Result<String> {
        let out = self
            .command()
            .stdin(Stdio::null())
            .output()
            .map_err(|e| self.error(e.to_string()))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(self.error(format!("{}: {}", out.status, stderr.trim())));
        }
        String::from_utf8(out.stdout).map_err(|e| Error::Parse(e.to_string()))
    }

    fn error(&self, detail: String) -> Error {
        Error::Command {
            command: self.display(),
            detail,
        }
    }
}
