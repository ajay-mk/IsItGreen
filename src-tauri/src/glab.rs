//! Reads the GitLab CLI's token by running
//! `glab auth credential-helper --repo https://<host>/isitgreen/link`.
//!
//! The rules for starting `glab` are in `cli.rs`. Here:
//!
//! - `glab` is only looked for in a fixed list of install locations.
//! - The arguments are fixed; the only variable part is a [`Host`] that has
//!   passed [`Host::parse`]. The `--repo` URL picks the instance.
//! - `GITLAB_TOKEN`, `GLAB_CONFIG_DIR`, `GITLAB_HOST` and friends never reach
//!   `glab`. Proxy and CA certificate settings do, since an OAuth token may
//!   be refreshed during the call.
//! - `glab` runs in the filesystem root with `GIT_DIR` set to a path that
//!   does not exist, so it finds no repository and reads no repo-local
//!   config from wherever IsItGreen was started.
//! - The answer is a JSON line. Only a success for the asked instance with a
//!   personal access or OAuth token is accepted. `glab`'s own messages are
//!   never shown.
//!
//! `glab` refreshes an expired OAuth token itself and saves the new refresh
//! token in its own config, so this call can write there. IsItGreen never
//! stores the token it gets this way; see `token.rs`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::cli::is_trusted;
use crate::error::{Error, Result};
use crate::gitlab::Host;

/// Longer than gh's limit, since `glab` may be refreshing an OAuth token.
const TIMEOUT: Duration = Duration::from_secs(20);

/// Network settings passed through for the refresh request, in both cases.
const PROXY_ENV: &[&str] = &[
    "HTTPS_PROXY",
    "HTTP_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "https_proxy",
    "http_proxy",
    "no_proxy",
    "all_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

/// A token from `glab`, and when it stops working if `glab` knows.
pub struct GlabToken {
    pub token: String,
    pub expires: Option<SystemTime>,
}

/// Where the GitLab CLI's installers put it, in the order they are tried.
fn candidates() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        for p in [
            "/opt/homebrew/bin/glab",
            "/usr/local/bin/glab",
            "/opt/local/bin/glab",
        ] {
            paths.push(p.into());
        }
    } else if cfg!(windows) {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        if let Some(pf) = var("ProgramFiles") {
            paths.push(pf.join("glab").join("glab.exe"));
        }
        if let Some(local) = var("LOCALAPPDATA") {
            paths.push(
                local
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Links")
                    .join("glab.exe"),
            );
        }
        if let Some(home) = var("USERPROFILE") {
            paths.push(home.join("scoop").join("shims").join("glab.exe"));
        }
        // Not Chocolatey's %ProgramData%\chocolatey\bin, for the reason given
        // in gh.rs.
    } else {
        for p in [
            "/usr/bin/glab",
            "/usr/local/bin/glab",
            "/home/linuxbrew/.linuxbrew/bin/glab",
            "/snap/bin/glab",
            "/run/current-system/sw/bin/glab",
        ] {
            paths.push(p.into());
        }
    }
    if cfg!(unix) {
        // Nix, on macOS and Linux alike.
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join(".nix-profile/bin/glab"));
        }
        paths.push("/nix/var/nix/profiles/default/bin/glab".into());
    }
    paths
}

/// Every place IsItGreen looks for glab on this system, for the setup
/// instructions.
pub fn searched() -> Vec<String> {
    candidates()
        .iter()
        .map(|p| p.display().to_string())
        .collect()
}

/// The GitLab CLI binary IsItGreen would run, if one is installed in a known
/// place.
pub fn locate() -> Option<PathBuf> {
    candidates()
        .into_iter()
        .find(|p| p.is_file() && is_trusted(p))
}

/// The directory `glab` runs in: one that ordinary users cannot write to,
/// so no repository or `.git` folder can be planted there.
fn root() -> PathBuf {
    if cfg!(windows) {
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
    } else {
        PathBuf::from("/")
    }
}

fn command(glab: &Path, host: &Host) -> Command {
    let repo = format!("https://{}/isitgreen/link", host.as_str());
    let root = root();
    let mut cmd = crate::cli::base_command(
        glab,
        &["auth", "credential-helper", "--repo", &repo],
        &[
            ("GLAB_NO_PROMPT", "1"),
            ("GLAB_CHECK_UPDATE", "false"),
            ("GLAB_SEND_TELEMETRY", "false"),
            ("NO_COLOR", "1"),
        ],
        Some(&root),
    );
    // glab asks `git rev-parse` for the repository; with GIT_DIR pointing
    // nowhere, git finds none and glab reads no repo-local config.
    cmd.env("GIT_DIR", root.join(".isitgreen-no-repo"));
    for name in PROXY_ENV {
        if let Some(value) = std::env::var_os(name) {
            cmd.env(name, value);
        }
    }
    cmd
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Reply {
    Success {
        instance_url: String,
        token: ReplyToken,
    },
    Error {
        // Only since glab 1.127.
        #[serde(default)]
        code: Option<String>,
        #[serde(default)]
        oauth_error: Option<String>,
        #[serde(default)]
        message: String,
    },
}

#[derive(Deserialize)]
struct ReplyToken {
    #[serde(rename = "type")]
    kind: String,
    token: String,
    #[serde(default)]
    expiry_timestamp: Option<String>,
}

/// Why `glab` gave no token. Only `not_authenticated` and a refresh the
/// server refused (`invalid_grant`) mean signing in again helps; anything
/// else (a locked keychain, a network error) is a generic failure. Without
/// a `code` (glab before 1.127), only the "not authenticated" message is
/// recognised.
fn failure(code: Option<&str>, oauth_error: Option<&str>, message: &str) -> Error {
    if no_login(code, message)
        || (code == Some("refresh_failed") && oauth_error == Some("invalid_grant"))
    {
        Error::GlabNotLoggedIn
    } else {
        Error::GlabFailed
    }
}

/// Whether glab says it has no credentials at all for the host, as opposed
/// to having some it could not use.
fn no_login(code: Option<&str>, message: &str) -> bool {
    match code {
        Some(code) => code == "not_authenticated",
        None => message.to_ascii_lowercase().contains("not authenticated"),
    }
}

/// Runs `glab auth credential-helper` for `host` and returns the token.
/// Blocking: call from a blocking-safe context.
pub fn token(host: &Host) -> Result<GlabToken> {
    let glab = locate().ok_or(Error::GlabNotFound)?;
    read(&glab, host)
}

/// A host no one can sign in to (`.invalid` is reserved), asked after every
/// success to see whether the token is really for `host`.
const PROBE_HOST: &str = "isitgreen-probe.invalid";

/// Asks `glab` for `host`'s token, then for [`PROBE_HOST`]'s. glab answers
/// for any host it has no login for with a top-level `token:` from its
/// config, and reports the asked host as the instance. If the probe gets
/// the same token, the token was not issued for `host`, so it is refused.
/// Both calls share one [`TIMEOUT`].
fn read(glab: &Path, host: &Host) -> Result<GlabToken> {
    let start = Instant::now();
    let found = run(command(glab, host), host)?;
    let probe = Host::parse(PROBE_HOST).map_err(|_| Error::Internal)?;
    let left = TIMEOUT
        .checked_sub(start.elapsed())
        .filter(|left| !left.is_zero())
        .ok_or(Error::GlabFailed)?;
    // Only a probe that finds no credentials at all, or a different token,
    // shows the token is the host's own; any other answer keeps it unused.
    match ask(command(glab, &probe), &probe, left) {
        Ok(Some(other)) if other.token == found.token => Err(Error::GlabNotLoggedIn),
        Ok(_) => Ok(found),
        Err(_) => Err(Error::GlabFailed),
    }
}

/// Runs a prepared `glab` command for `host` and returns the token.
fn run(cmd: Command, host: &Host) -> Result<GlabToken> {
    ask(cmd, host, TIMEOUT)?.ok_or(Error::GlabNotLoggedIn)
}

/// Like [`run`], within `timeout`, but `Ok(None)` when glab has no
/// credentials for `host`.
fn ask(cmd: Command, host: &Host, timeout: Duration) -> Result<Option<GlabToken>> {
    let out = crate::cli::run(cmd, timeout).map_err(|_| Error::GlabFailed)?;
    // Errors in finding a token exit 0 with a JSON error; a non-zero exit is
    // a command line this glab does not accept.
    if !out.status_ok {
        let stderr = String::from_utf8_lossy(&out.stderr).to_ascii_lowercase();
        return Err(
            if stderr.contains("unknown command") || stderr.contains("unknown flag") {
                Error::GlabOutdated
            } else {
                Error::GlabFailed
            },
        );
    }
    // glab before 1.85 has no `credential-helper` and prints the help for
    // `glab auth` instead, exiting 0.
    if !out.stdout.trim_ascii_start().starts_with(b"{") {
        return Err(Error::GlabOutdated);
    }
    let reply: Reply = serde_json::from_slice(&out.stdout).map_err(|_| Error::GlabFailed)?;
    let (instance_url, token) = match reply {
        Reply::Success {
            instance_url,
            token,
        } => (instance_url, token),
        Reply::Error {
            code,
            oauth_error,
            message,
        } => {
            let code = code.filter(|c| !c.is_empty());
            if no_login(code.as_deref(), &message) {
                return Ok(None);
            }
            return Err(failure(code.as_deref(), oauth_error.as_deref(), &message));
        }
    };
    if Host::parse(&instance_url).ok().as_ref() != Some(host) {
        return Err(Error::GlabFailed);
    }
    // A CI job token is scoped to one job and not a user's own login.
    if token.kind != "pat" && token.kind != "oauth2" {
        return Err(Error::GlabFailed);
    }
    crate::token::validate_format(crate::token::Provider::GitLab, &token.token)
        .map_err(|_| Error::GlabFailed)?;
    let expires = match token.expiry_timestamp {
        Some(ts) => Some(parse_timestamp(&ts).ok_or(Error::GlabFailed)?),
        None => None,
    };
    Ok(Some(GlabToken {
        token: token.token,
        expires,
    }))
}

/// Parses the RFC 3339 time glab writes, such as `2026-11-26T18:28:54.123Z`.
/// A time `SystemTime` cannot hold is refused rather than panicking.
fn parse_timestamp(s: &str) -> Option<SystemTime> {
    use time::format_description::well_known::Rfc3339;
    let secs = time::OffsetDateTime::parse(s, &Rfc3339)
        .ok()?
        .unix_timestamp();
    match u64::try_from(secs) {
        Ok(after) => UNIX_EPOCH.checked_add(Duration::from_secs(after)),
        Err(_) => UNIX_EPOCH.checked_sub(Duration::from_secs(secs.unsigned_abs())),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::time::{Duration, UNIX_EPOCH};

    use crate::cli::test_support::{fake_bin, serial};

    const PAT: &str = "glpat-0123456789abcdefghij";
    const OAUTH: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c4b5a69788796a5b4c3d2e1f0";
    const MARKER: &str = "GLAB-SAID-THIS";

    fn host() -> Host {
        Host::parse("gitlab.example.com:8443").unwrap()
    }

    /// A stand-in for `glab` in a fresh private directory. The default
    /// script records its arguments, working directory and environment, then
    /// prints whatever the test last put in `out` and `err` and exits with
    /// `code`, so one binary serves many answers without being rewritten.
    struct Fake {
        dir: PathBuf,
        bin: PathBuf,
    }

    impl Fake {
        fn with_body(name: &str, body: &str) -> Fake {
            let bin = fake_bin("glab", "glab", name, body);
            let dir = bin.parent().unwrap().to_path_buf();
            let fake = Fake { dir, bin };
            fake.reply("", "", 0);
            fake
        }

        fn new(name: &str) -> Fake {
            Fake::with_body(
                name,
                "DIR=$(dirname \"$0\")\n\
                 printf '%s\\n' \"$*\" > \"$DIR/args\"\n\
                 pwd -P > \"$DIR/cwd\"\n\
                 env > \"$DIR/env\"\n\
                 cat \"$DIR/out\"\n\
                 cat \"$DIR/err\" >&2\n\
                 exit \"$(cat \"$DIR/code\")\"",
            )
        }

        fn reply(&self, stdout: &str, stderr: &str, code: i32) {
            std::fs::write(self.dir.join("out"), stdout).unwrap();
            std::fs::write(self.dir.join("err"), stderr).unwrap();
            std::fs::write(self.dir.join("code"), code.to_string()).unwrap();
        }

        fn reply_json(&self, answer: &Value) {
            self.reply(&format!("{answer}\n"), "", 0);
        }

        fn seen(&self, what: &str) -> String {
            std::fs::read_to_string(self.dir.join(what)).unwrap_or_default()
        }

        fn seen_env(&self) -> std::collections::HashMap<String, String> {
            self.seen("env")
                .lines()
                .filter_map(|l| l.split_once('='))
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect()
        }

        fn ask(&self, host: &Host) -> Result<GlabToken> {
            run(command(&self.bin, host), host)
        }
    }

    fn success(instance_url: &str, kind: &str, token: &str) -> Value {
        json!({
            "type": "success",
            "instance_url": instance_url,
            "token": { "type": kind, "token": token },
        })
    }

    #[test]
    fn asks_for_the_host_with_fixed_arguments_outside_any_repository() {
        let _serial = serial();
        let fake = Fake::new("args");
        fake.reply_json(&success("https://gitlab.example.com:8443", "pat", PAT));
        let got = fake.ask(&host()).unwrap();
        assert_eq!(got.token, PAT);
        assert_eq!(got.expires, None);
        assert_eq!(
            fake.seen("args").trim_end(),
            "auth credential-helper --repo https://gitlab.example.com:8443/isitgreen/link"
        );
        assert_eq!(fake.seen("cwd").trim_end(), "/");
        // glab runs `git rev-parse`; GIT_DIR points at nothing, so no
        // repository (and no repo-local glab config) is found.
        let env = fake.seen_env();
        let git_dir = env.get("GIT_DIR").expect("GIT_DIR is set");
        assert!(Path::new(git_dir).is_absolute(), "{git_dir}");
        assert!(!Path::new(git_dir).exists(), "{git_dir}");
    }

    #[test]
    fn settings_that_change_the_answer_never_reach_glab() {
        let _serial = serial();
        const CLEARED: &[&str] = &[
            "GITLAB_TOKEN",
            "GITLAB_ACCESS_TOKEN",
            "OAUTH_TOKEN",
            "GLAB_CONFIG_DIR",
            "GITLAB_HOST",
            "GITLAB_URI",
            "GL_HOST",
            "GITLAB_REPO",
            "GLAB_USE_KEYRING",
            "CI",
            "GITLAB_CI",
            "GLAB_ENABLE_CI_AUTOLOGIN",
            "GH_TOKEN",
        ];
        // Needed to reach a self-hosted instance behind a proxy or private CA.
        const PASSED: &[&str] = &[
            "HTTPS_PROXY",
            "HTTP_PROXY",
            "NO_PROXY",
            "ALL_PROXY",
            "https_proxy",
            "http_proxy",
            "no_proxy",
            "all_proxy",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
        ];
        let saved: Vec<_> = CLEARED
            .iter()
            .chain(PASSED)
            .map(|n| (*n, std::env::var_os(n)))
            .collect();
        for name in CLEARED {
            std::env::set_var(name, format!("from-env-{name}"));
        }
        for name in PASSED {
            std::env::set_var(name, format!("passed-{name}"));
        }
        let fake = Fake::new("env");
        fake.reply_json(&success("https://gitlab.example.com:8443", "pat", PAT));
        let result = fake.ask(&host());
        for (name, value) in saved {
            match value {
                Some(v) => std::env::set_var(name, v),
                None => std::env::remove_var(name),
            }
        }
        assert_eq!(result.unwrap().token, PAT);
        let env = fake.seen_env();
        for name in CLEARED {
            assert_eq!(env.get(*name), None, "{name} reached glab");
        }
        for name in PASSED {
            assert_eq!(env.get(*name), Some(&format!("passed-{name}")), "{name}");
        }
        for (name, value) in [
            ("PATH", "/usr/bin:/bin:/usr/sbin:/sbin"),
            ("GLAB_NO_PROMPT", "1"),
            ("GLAB_CHECK_UPDATE", "false"),
            ("GLAB_SEND_TELEMETRY", "false"),
            ("NO_COLOR", "1"),
        ] {
            assert_eq!(env.get(name).map(String::as_str), Some(value), "{name}");
        }
    }

    #[test]
    fn reads_the_expiry_of_an_oauth_token() {
        let _serial = serial();
        let fake = Fake::new("oauth");
        let at = |s| UNIX_EPOCH + Duration::from_secs(s);
        let mut answer = success("https://gitlab.example.com:8443", "oauth2", OAUTH);
        answer["token"]["expiry_timestamp"] = json!("2030-01-02T03:04:05Z");
        fake.reply_json(&answer);
        let got = fake.ask(&host()).unwrap();
        assert_eq!(got.token, OAUTH);
        assert_eq!(got.expires, Some(at(1_893_553_445)));
        // Go writes sub-second digits whenever there are any, as there are
        // for a token that was just refreshed.
        answer["token"]["expiry_timestamp"] = json!("2030-01-02T03:04:05.123456789Z");
        fake.reply_json(&answer);
        let expires = fake.ask(&host()).unwrap().expires.unwrap();
        assert!(
            (at(1_893_553_445)..=at(1_893_553_446)).contains(&expires),
            "{expires:?}"
        );
    }

    #[test]
    fn the_instance_is_compared_as_a_host_not_as_text() {
        let _serial = serial();
        let fake = Fake::new("parsed-host");
        fake.reply_json(&success("https://GitLab.Example.com:8443/", "pat", PAT));
        assert_eq!(fake.ask(&host()).unwrap().token, PAT);
        let com = Host::parse("gitlab.com").unwrap();
        fake.reply_json(&success("https://gitlab.com:443", "pat", PAT));
        assert_eq!(fake.ask(&com).unwrap().token, PAT);
    }

    #[test]
    fn refuses_anything_but_a_usable_token_for_the_host() {
        let _serial = serial();
        let fake = Fake::new("refuse");
        let base = success("https://gitlab.example.com:8443", "pat", PAT);
        // The unchanged answer is accepted, so each refusal below is caused
        // by the one field it changes.
        fake.reply_json(&base);
        assert_eq!(fake.ask(&host()).unwrap().token, PAT);

        type Change = fn(&mut Value);
        let cases: Vec<(&str, Change)> = vec![
            ("CI job token", |v| v["token"]["type"] = json!("job-token")),
            ("unknown token type", |v| {
                v["token"]["type"] = json!("other")
            }),
            ("other instance", |v| {
                v["instance_url"] = json!("https://other.example.com:8443")
            }),
            ("with a path", |v| {
                v["instance_url"] = json!("https://gitlab.example.com:8443/evil")
            }),
            ("short token", |v| v["token"]["token"] = json!("glpat-x")),
            ("bad expiry", |v| {
                v["token"]["expiry_timestamp"] = json!("next tuesday")
            }),
        ];
        for (name, change) in cases {
            let mut answer = base.clone();
            change(&mut answer);
            fake.reply_json(&answer);
            assert!(
                matches!(fake.ask(&host()), Err(Error::GlabFailed)),
                "{name}"
            );
        }
    }

    #[test]
    fn maps_glab_errors_to_fixed_messages() {
        let _serial = serial();
        let fake = Fake::new("errors");
        let error = |fields: Value| {
            let mut v = json!({ "type": "error", "host": "gitlab.example.com:8443" });
            v.as_object_mut()
                .unwrap()
                .extend(fields.as_object().unwrap().clone());
            v
        };
        let not_auth = "glab is not authenticated. Use glab auth login to authenticate";
        let cases = [
            (
                "not signed in",
                json!({ "code": "not_authenticated", "message": MARKER }),
                "glab_not_logged_in",
            ),
            (
                "glab before 1.127, not signed in",
                json!({ "message": format!("{MARKER} {not_auth}") }),
                "glab_not_logged_in",
            ),
            (
                "glab before 1.127, other",
                json!({ "message": MARKER }),
                "glab_failed",
            ),
            (
                "refresh token revoked",
                json!({ "code": "refresh_failed", "oauth_error": "invalid_grant", "message": MARKER }),
                "glab_not_logged_in",
            ),
            (
                "refresh failed, no OAuth error",
                json!({ "code": "refresh_failed", "message": MARKER }),
                "glab_failed",
            ),
            (
                "refresh failed, server trouble",
                json!({ "code": "refresh_failed", "oauth_error": "temporarily_unavailable", "message": MARKER }),
                "glab_failed",
            ),
            // With a code, the message is not consulted.
            (
                "keyring locked",
                json!({ "code": "keyring_unavailable", "message": format!("{MARKER} {not_auth}") }),
                "glab_failed",
            ),
            (
                "unknown",
                json!({ "code": "unknown", "message": MARKER }),
                "glab_failed",
            ),
            (
                "empty code",
                json!({ "code": "", "message": format!("{MARKER} {not_auth}") }),
                "glab_not_logged_in",
            ),
        ];
        for (name, fields, kind) in cases {
            fake.reply_json(&error(fields));
            let err = fake
                .ask(&host())
                .err()
                .unwrap_or_else(|| panic!("{name}: accepted"));
            assert_eq!(serde_json::to_value(&err).unwrap()["kind"], kind, "{name}");
            assert!(
                !format!("{err} {err:?}").contains(MARKER),
                "{name}: {err:?}"
            );
        }
    }

    #[test]
    fn a_glab_without_the_credential_helper_is_outdated() {
        let _serial = serial();
        let fake = Fake::new("old");
        for stderr in [
            "Error: unknown command \"credential-helper\" for \"glab auth\"",
            "Error: unknown flag: --repo",
        ] {
            fake.reply("", stderr, 1);
            assert!(
                matches!(fake.ask(&host()), Err(Error::GlabOutdated)),
                "{stderr}"
            );
        }
        // glab before 1.85 prints the help for `glab auth` and exits 0.
        for stdout in [
            "",
            "Manage glab's authentication state.\n\nUSAGE\n  glab auth <command>\n",
        ] {
            fake.reply(stdout, "", 0);
            assert!(
                matches!(fake.ask(&host()), Err(Error::GlabOutdated)),
                "{stdout:?}"
            );
        }
        fake.reply("", &format!("{MARKER}: something else broke"), 1);
        let err = fake.ask(&host()).err().unwrap();
        assert!(matches!(err, Error::GlabFailed));
        assert!(!format!("{err} {err:?}").contains(MARKER));
    }

    /// A stand-in for `glab` that answers by the `--repo` host:
    /// `for_probe` for [`PROBE_HOST`], `for_host` for any other.
    fn by_host(name: &str, for_host: &Value, for_probe: &Value) -> Fake {
        Fake::with_body(
            name,
            &format!(
                "case \"$4\" in\n  https://{PROBE_HOST}/*) echo '{for_probe}' ;;\n  *) echo '{for_host}' ;;\nesac"
            ),
        )
    }

    #[test]
    fn a_token_glab_gives_for_every_host_is_refused() {
        let _serial = serial();
        // A top-level `token:` in glab's config, with no `hosts:` entry.
        let fake = by_host(
            "hostless",
            &success("https://gitlab.example.com:8443", "pat", PAT),
            &success(&format!("https://{PROBE_HOST}"), "pat", PAT),
        );
        assert!(matches!(
            read(&fake.bin, &host()),
            Err(Error::GlabNotLoggedIn)
        ));
    }

    #[test]
    fn a_login_for_the_host_wins_over_a_top_level_token() {
        let _serial = serial();
        let fake = by_host(
            "per-host",
            &success("https://gitlab.example.com:8443", "pat", PAT),
            &success(
                &format!("https://{PROBE_HOST}"),
                "pat",
                "glpat-toplevel0123456789ab",
            ),
        );
        assert_eq!(read(&fake.bin, &host()).unwrap().token, PAT);
    }

    #[test]
    fn a_probe_that_finds_no_login_accepts_the_token() {
        let _serial = serial();
        let fake = by_host(
            "probe-error",
            &success("https://gitlab.example.com:8443", "pat", PAT),
            &json!({
                "type": "error",
                "code": "not_authenticated",
                "host": PROBE_HOST,
                "message": "glab is not authenticated. Use glab auth login to authenticate",
            }),
        );
        assert_eq!(read(&fake.bin, &host()).unwrap().token, PAT);
    }

    #[test]
    fn a_probe_that_fails_otherwise_keeps_the_token_unused() {
        let _serial = serial();
        let fake = by_host(
            "probe-keyring",
            &success("https://gitlab.example.com:8443", "pat", PAT),
            &json!({
                "type": "error",
                "code": "keyring_unavailable",
                "host": PROBE_HOST,
                "message": "could not read the keyring",
            }),
        );
        assert!(matches!(read(&fake.bin, &host()), Err(Error::GlabFailed)));
    }

    #[test]
    fn a_probe_whose_refresh_was_refused_keeps_the_token_unused() {
        let _serial = serial();
        let fake = by_host(
            "probe-refresh",
            &success("https://gitlab.example.com:8443", "pat", PAT),
            &json!({
                "type": "error",
                "code": "refresh_failed",
                "oauth_error": "invalid_grant",
                "host": PROBE_HOST,
                "message": "failed to read the credentials",
            }),
        );
        assert!(matches!(read(&fake.bin, &host()), Err(Error::GlabFailed)));
    }

    #[test]
    fn only_known_install_locations_are_tried() {
        for path in candidates() {
            assert!(path.is_absolute(), "{path:?}");
            assert!(
                path.ends_with("glab") || path.ends_with("glab.exe"),
                "{path:?}"
            );
        }
    }
}
