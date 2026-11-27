//! Reads the GitHub CLI's token by running `gh auth token`.
//!
//! The rules for starting `gh` are in `cli.rs`. Here:
//!
//! - `gh` is only looked for in a fixed list of install locations.
//! - The arguments are fixed; the only variable part is a login that has
//!   passed [`is_valid_login`].
//! - `GH_TOKEN`, `GITHUB_TOKEN`, `GH_CONFIG_DIR` and friends never reach
//!   `gh`, so they cannot change which token comes back.
//!
//! IsItGreen never stores the token it gets this way; see `token.rs`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::cli::is_trusted;
use crate::error::{Error, Result};
use crate::gitlab::is_valid_login;

const TIMEOUT: Duration = Duration::from_secs(10);

/// Where the GitHub CLI's installers put it, in the order they are tried.
fn candidates() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        for p in [
            "/opt/homebrew/bin/gh",
            "/usr/local/bin/gh",
            "/opt/local/bin/gh",
        ] {
            paths.push(p.into());
        }
    } else if cfg!(windows) {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        if let Some(pf) = var("ProgramFiles") {
            paths.push(pf.join("GitHub CLI").join("gh.exe"));
        }
        if let Some(local) = var("LOCALAPPDATA") {
            paths.push(
                local
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Links")
                    .join("gh.exe"),
            );
        }
        if let Some(home) = var("USERPROFILE") {
            paths.push(home.join("scoop").join("shims").join("gh.exe"));
        }
        // Not Chocolatey's %ProgramData%\chocolatey\bin: where Chocolatey is
        // not installed, any local user may create that folder, and a gh.exe
        // planted there would run in the account of whoever links gh.
    } else {
        for p in [
            "/usr/bin/gh",
            "/usr/local/bin/gh",
            "/home/linuxbrew/.linuxbrew/bin/gh",
            "/snap/bin/gh",
            "/run/current-system/sw/bin/gh",
        ] {
            paths.push(p.into());
        }
    }
    if cfg!(unix) {
        // Nix, on macOS and Linux alike.
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".nix-profile/bin/gh"));
        }
        paths.push("/nix/var/nix/profiles/default/bin/gh".into());
    }
    paths
}

/// Every place IsItGreen looks for gh on this system, for the setup
/// instructions.
pub fn searched() -> Vec<String> {
    candidates()
        .iter()
        .map(|p| p.display().to_string())
        .collect()
}

/// The GitHub CLI binary IsItGreen would run, if one is installed in a known
/// place.
pub fn locate() -> Option<PathBuf> {
    candidates()
        .into_iter()
        .find(|p| p.is_file() && is_trusted(p))
}

fn command(gh: &Path, login: Option<&str>) -> Command {
    let mut args = vec!["auth", "token", "--hostname", "github.com"];
    if let Some(login) = login {
        args.extend(["--user", login]);
    }
    crate::cli::base_command(
        gh,
        &args,
        &[
            ("GH_PROMPT_DISABLED", "1"),
            ("GH_NO_UPDATE_NOTIFIER", "1"),
            ("NO_COLOR", "1"),
        ],
        None,
    )
}

/// Why `gh` exited without a token, from its error output. Only known
/// phrases are recognised; anything else (a locked keychain, a broken
/// config) is a generic failure rather than advice to sign in again.
fn failure(stderr: &str) -> Error {
    let stderr = stderr.to_ascii_lowercase();
    if stderr.contains("no oauth token found") || stderr.contains("not logged in") {
        Error::GhNotLoggedIn
    } else if stderr.contains("unknown flag") || stderr.contains("unknown command") {
        Error::GhOutdated
    } else {
        Error::GhFailed
    }
}

/// Runs `gh auth token` for github.com, for `login` if given (otherwise gh's
/// active account), and returns the token. Blocking: call from a
/// blocking-safe context.
pub fn token(login: Option<&str>) -> Result<String> {
    let gh = locate().ok_or(Error::GhNotFound)?;
    if login.is_some_and(|l| !is_valid_login(l)) {
        return Err(Error::InvalidOwner);
    }
    run(command(&gh, login))
}

fn run(cmd: Command) -> Result<String> {
    let out = crate::cli::run(cmd, TIMEOUT).map_err(|_| Error::GhFailed)?;
    if !out.status_ok {
        return Err(failure(&String::from_utf8_lossy(&out.stderr)));
    }
    let token = String::from_utf8(out.stdout).map_err(|_| Error::GhFailed)?;
    let token = token.trim();
    crate::token::validate_format(crate::token::Provider::GitHub, token)
        .map_err(|_| Error::GhFailed)?;
    Ok(token.to_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    use crate::cli::test_support::serial;
    use std::time::Instant;

    /// A stand-in for `gh`: a shell script in a fresh private directory.
    fn fake_gh(name: &str, body: &str) -> PathBuf {
        crate::cli::test_support::fake_bin("gh", "gh", name, body)
    }

    const TOKEN: &str = "gho_0123456789abcdefghijABCDEFGHIJ0123";

    #[test]
    fn passes_fixed_arguments_and_returns_the_token() {
        let _serial = serial();
        let gh = fake_gh(
            "args",
            &format!(
                "[ \"$*\" = \"auth token --hostname github.com --user octo-cat\" ] || exit 9\necho {TOKEN}"
            ),
        );
        assert_eq!(run(command(&gh, Some("octo-cat"))).unwrap(), TOKEN);
    }

    #[test]
    fn environment_tokens_never_reach_gh() {
        let _serial = serial();
        std::env::set_var("GH_TOKEN", "ghp_fromtheenvironment000000000000000");
        std::env::set_var("GH_CONFIG_DIR", "/tmp/elsewhere");
        let gh = fake_gh(
            "env",
            &format!(
                "[ -z \"$GH_TOKEN$GITHUB_TOKEN$GH_CONFIG_DIR\" ] || exit 9\n[ \"$PATH\" = /usr/bin:/bin:/usr/sbin:/sbin ] || exit 7\n[ \"$GH_PROMPT_DISABLED\" = 1 ] || exit 8\necho {TOKEN}"
            ),
        );
        let result = run(command(&gh, None));
        std::env::remove_var("GH_TOKEN");
        std::env::remove_var("GH_CONFIG_DIR");
        assert_eq!(result.unwrap(), TOKEN);
    }

    #[test]
    fn maps_failures_without_echoing_them() {
        let _serial = serial();
        let logged_out = fake_gh(
            "logged-out",
            "echo 'no oauth token found for github.com' >&2\nexit 1",
        );
        assert!(matches!(
            run(command(&logged_out, None)),
            Err(Error::GhNotLoggedIn)
        ));
        let old = fake_gh("old", "echo 'unknown flag: --user' >&2\nexit 1");
        assert!(matches!(
            run(command(&old, Some("me"))),
            Err(Error::GhOutdated)
        ));
        let removed = fake_gh(
            "removed",
            "echo 'unknown command \"token\" for \"gh auth\"' >&2\nexit 1",
        );
        assert!(matches!(
            run(command(&removed, None)),
            Err(Error::GhOutdated)
        ));
        // Anything else is not a reason to tell people to sign in again.
        let keyring = fake_gh("keyring", "echo 'failed to unlock keyring' >&2\nexit 1");
        assert!(matches!(run(command(&keyring, None)), Err(Error::GhFailed)));
        let garbage = fake_gh("garbage", "echo 'not a token at all'");
        assert!(matches!(run(command(&garbage, None)), Err(Error::GhFailed)));
    }

    #[test]
    fn stops_a_hanging_gh() {
        let _serial = serial();
        let slow = fake_gh("slow", "exec sleep 30");
        let start = Instant::now();
        assert!(matches!(run(command(&slow, None)), Err(Error::GhFailed)));
        assert!(start.elapsed() < TIMEOUT + Duration::from_secs(5));
    }

    #[test]
    fn a_helper_holding_the_output_open_cannot_outlast_the_limit() {
        let _serial = serial();
        // gh exits at once, but a background process keeps stdout open.
        let leaky = fake_gh("leaky", &format!("sleep 30 &\necho {TOKEN}"));
        let start = Instant::now();
        assert!(matches!(run(command(&leaky, None)), Err(Error::GhFailed)));
        assert!(start.elapsed() < TIMEOUT + Duration::from_secs(5));
    }

    #[test]
    fn caps_output() {
        let _serial = serial();
        let chatty = fake_gh(
            "chatty",
            "yes gho_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa | head -c 100000",
        );
        assert!(matches!(run(command(&chatty, None)), Err(Error::GhFailed)));
    }

    #[test]
    fn refuses_binaries_others_can_replace() {
        let _serial = serial();
        let gh = fake_gh("trust", "true");
        assert!(is_trusted(&gh));
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(!is_trusted(&gh));
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
        let dir = gh.parent().unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(!is_trusted(&gh));
        assert!(!is_trusted(&dir.join("missing")));
    }

    #[test]
    fn only_known_install_locations_are_tried() {
        let _serial = serial();
        for path in candidates() {
            assert!(path.is_absolute(), "{path:?}");
            assert!(path.ends_with("gh") || path.ends_with("gh.exe"));
        }
    }
}
