//! Runs a provider's command-line tool (`gh` or `glab`) to read its token.
//!
//! These are the only places the backend starts another program, so the
//! runner is kept narrow:
//!
//! - The caller finds the binary in a fixed list of install locations (never
//!   on `PATH`, never at a path the frontend names), and [`is_trusted`]
//!   refuses one in a directory anyone can write to.
//! - The arguments are fixed by the caller. No shell is involved.
//! - The child gets a minimal environment ([`PASSED_ENV`] plus what the
//!   caller adds), so token and config variables in our own environment
//!   cannot change which token comes back.
//! - It has no stdin, a time limit and a cap on what it may print. Callers
//!   only match its error output against known phrases and never show it,
//!   since it could in theory contain a token.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The most a tool may print on stdout. Longer output is refused; stderr is
/// cut at this length.
pub const MAX_OUTPUT: u64 = 4096;

/// A file or directory others may write to could have been swapped for
/// something else, so neither the binary nor its directory may be writable
/// by everyone. Symlinks (Homebrew links into its Cellar) are checked at
/// both ends.
#[cfg(unix)]
pub fn is_trusted(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let world_writable =
        |p: &Path| std::fs::metadata(p).map_or(true, |m| m.permissions().mode() & 0o002 != 0);
    let Ok(real) = std::fs::canonicalize(path) else {
        return false;
    };
    let mut checked = vec![path.to_path_buf(), real.clone()];
    checked.extend(path.parent().map(Path::to_path_buf));
    checked.extend(real.parent().map(Path::to_path_buf));
    real.is_file() && !checked.iter().any(|p| world_writable(p))
}

#[cfg(not(unix))]
pub fn is_trusted(path: &Path) -> bool {
    path.is_file()
}

/// Variables the tools need to find their config and the OS keychain.
/// Everything else, including any `GH_*`, `GITHUB_*`, `GLAB_*` or `GITLAB_*`
/// setting, is dropped.
pub const PASSED_ENV: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
    "SystemRoot",
    "windir",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "HOMEDRIVE",
    "HOMEPATH",
    "TEMP",
    "TMP",
];

/// A command for `bin` with `args`, a cleared environment holding only
/// [`PASSED_ENV`] and `extra_env`, no stdin, and piped output. With `cwd`
/// `None` the child starts in our own working directory.
pub fn base_command(
    bin: &Path,
    args: &[&str],
    extra_env: &[(&str, &str)],
    cwd: Option<&Path>,
) -> Command {
    let mut cmd = Command::new(bin);
    cmd.args(args);
    cmd.env_clear();
    // A fixed search path for anything the tool itself runs (macOS's
    // `security`, `git`).
    if cfg!(unix) {
        cmd.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    }
    for name in PASSED_ENV {
        if let Some(value) = std::env::var_os(name) {
            cmd.env(name, value);
        }
    }
    cmd.envs(extra_env.iter().copied());
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // A console program started from a GUI app would flash a console window.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// What a finished tool printed, and whether it exited with success.
pub struct Output {
    pub status_ok: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Runs `cmd` for at most `timeout`. Fails if it cannot start, does not
/// finish in time, or prints more than [`MAX_OUTPUT`] on stdout. Blocking.
pub fn run(mut cmd: Command, timeout: Duration) -> Result<Output, ()> {
    let mut child = cmd.spawn().map_err(|_| ())?;
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        return Err(());
    };
    // Read both pipes while waiting, so a chatty child cannot block on a full
    // pipe; anything past the cap is ignored. The results come back over a
    // channel, so a helper process that keeps a pipe open after the tool
    // exits cannot hold us past the time limit.
    let read = |pipe: Box<dyn Read + Send>| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.take(MAX_OUTPUT + 1).read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
        rx
    };
    let out = read(Box::new(stdout));
    let err = read(Box::new(stderr));

    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(25));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(());
            }
        }
    };
    let collect = |rx: std::sync::mpsc::Receiver<Vec<u8>>| {
        let left = timeout
            .saturating_sub(start.elapsed())
            .max(Duration::from_millis(500));
        rx.recv_timeout(left).map_err(|_| ())
    };
    let stdout = collect(out)?;
    let stderr = collect(err)?;
    if stdout.len() as u64 > MAX_OUTPUT {
        return Err(());
    }
    Ok(Output {
        status_ok: status.success(),
        stdout,
        stderr,
    })
}

#[cfg(all(test, unix))]
pub mod test_support {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    /// Writing an executable while another test thread forks can make the
    /// exec fail with "text file busy", so tests that start fake tools run
    /// one at a time.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    pub fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A stand-in for a tool: a shell script named `bin` in a fresh private
    /// directory whose name contains `tag` and `name`.
    pub fn fake_bin(tag: &str, bin: &str, name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "isitgreen-{tag}-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join(bin);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
}
