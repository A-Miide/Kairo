//! Programs Kairo starts (ffmpeg, tar, the installers). On Windows the app has no console of its
//! own (main.rs, windows_subsystem), so a program that has one opens a console window on screen
//! each time it starts: these start them without (CREATE_NO_WINDOW). Everywhere else they are
//! the standard commands.

use std::ffi::OsStr;
use std::ops::{Deref, DerefMut};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `std::process::Command`, started without a console window on Windows.
pub struct Command(std::process::Command);

impl Command {
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        #[allow(unused_mut)]
        let mut cmd = std::process::Command::new(program);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        Command(cmd)
    }
}

impl Deref for Command {
    type Target = std::process::Command;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Command {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// `tokio::process::Command`, started without a console window on Windows.
pub struct AsyncCommand(tokio::process::Command);

impl AsyncCommand {
    pub fn new<S: AsRef<OsStr>>(program: S) -> Self {
        #[allow(unused_mut)]
        let mut cmd = tokio::process::Command::new(program);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);
        AsyncCommand(cmd)
    }
}

impl Deref for AsyncCommand {
    type Target = tokio::process::Command;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for AsyncCommand {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `echo` is a program everywhere but Windows, where it's built into cmd.
    #[cfg(windows)]
    const ECHO: (&str, &[&str]) = ("cmd", &["/C", "echo", "kairo"]);
    #[cfg(not(windows))]
    const ECHO: (&str, &[&str]) = ("echo", &["kairo"]);

    #[test]
    fn runs_programs_and_gives_their_output() {
        let out = Command::new(ECHO.0).args(ECHO.1).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "kairo");
    }

    #[tokio::test]
    async fn runs_programs_asynchronously_too() {
        let out = AsyncCommand::new(ECHO.0).args(ECHO.1).output().await.unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "kairo");
    }
}
