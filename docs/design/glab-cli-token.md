# Use the GitLab CLI's token

Status: implemented as a live link, as for GitHub. This note covers only what
differs from [gh-cli-token.md](gh-cli-token.md); read that for the reasoning
about a live link versus a one-time import, which applies here unchanged.

## Decisions

- **Built-in queries first.** glab's token carries the write-capable `api`
  scope. As for GitHub before the gh link, the frontend therefore lost the
  ability to send GitLab query text before glab was linked. It names one of
  six operations and passes variables, and the backend builds the document
  (`src-tauri/src/gitlab_queries.rs`). Unlike GitHub's operations, every
  GitLab operation's variables are checked against a fixed shape: known keys
  only, bounded sizes and counts.
- **Live link, no stored token.** The default GitLab slot can be linked to
  glab. The index records the instance and the login, and the token is read
  from glab when needed and kept in memory only. Linking deletes any stored
  default token.
- **Which glab command.** `glab auth credential-helper --repo
  https://<host>/isitgreen/link`. It is hidden but exists since glab 1.85
  (the minimum version), prints one JSON line on stdout and exits 0 even for
  credential errors. The `--repo` URL picks the instance, whatever the
  working directory is.
- **The runner is shared.** What `gh.rs` did to start a program moved to
  `src-tauri/src/cli.rs` without a change in behaviour for gh. glab adds a
  working directory (the filesystem root) and `GIT_DIR` set to a path that
  does not exist, so glab finds no repository and reads no repo-local config.
- **A longer time limit (20 s, gh has 10 s).** glab refreshes an expired
  OAuth token inside this call and saves the new refresh token, so the call
  can use the network and write to glab's config. For the same reason the
  proxy and certificate variables are passed on. Stopping glab at the limit in the middle of a refresh can sign the
  user out of glab.
- **What counts as a good reply.** A success whose `instance_url` parses to
  the instance asked for, a token type of `pat` or `oauth2` (a CI job token
  is refused), a token that looks like a GitLab token, and an expiry that
  parses if there is one. glab's messages are never shown. An error reply maps
  to "not signed in" only for `code` `not_authenticated`, or `refresh_failed`
  with `oauth_error` `invalid_grant`. Before glab 1.127 there is no `code`, so
  the message text "not authenticated" is matched instead. Any other error is
  a generic failure. glab configs that use `api_host`, or an instance under a
  subfolder, report the API host or path as `instance_url`; the instance
  check refuses it and the user sees the generic failure message. A token
  that glab keeps outside any host (`glab config set token --global`) would
  answer for every host, so IsItGreen also asks for a reserved `.invalid`
  host on every read. The same token for it means glab has no login for the
  instance, reported as not signed in. Signing in again does not help then;
  the global token has to be removed. A different token or "not
  authenticated" lets the token through, and any other failure keeps it
  unused. Output that is not JSON means a glab older than 1.85, which
  prints the help for `glab auth` and exits 0. An expired OAuth login that
  glab cannot renew (no refresh token) gives the generic failure, and signing
  in again fixes it.
- **Rechecks.** A cached token is reused for 60 seconds, the same as for gh,
  and not when it expires within 30 seconds (`GLAB_EXPIRY_MARGIN`). A token
  glab gave before is not checked with GitLab again. A different one is
  checked, and must belong to the linked login.
- **A revoked token is not noticed by the recheck.** The recheck asks glab
  again, but glab returns the same token, and the same token skips the GitLab
  check. GitLab answers a rejected token as an anonymous request, not with
  HTTP 401, so the 401 path that heals a gh link does not fire. The
  frontend's `requireViewer` (`src/providers/gitlab/index.ts`) turns the
  missing signed-in user into `unauthorized`, and the setup screen appears.
  Signing out of glab is noticed by the recheck, since glab then gives no
  token.
- **Setup order.** Both tabs of the setup screen show the CLI login first
  (a button when the CLI is found, setup steps and **Check again** when not),
  then the token form. The form stays visible, so the CLI is the first choice
  and not the only one. On the GitLab tab the host field sits above the CLI
  block and feeds both.
- **Error kinds.** `glab_not_found`, `glab_not_logged_in`, `glab_outdated`,
  `glab_failed` and `glab_account_mismatch`, separate from the gh kinds.

## What was built

- `src-tauri/src/gitlab_queries.rs`: the GitLab documents and variable
  checks. `gitlab_graphql` takes an operation name and variables.
- `src-tauri/src/cli.rs`: the shared runner and `is_trusted`.
- `src-tauri/src/glab.rs`: finds glab (Homebrew, MacPorts, Linux packages,
  Linuxbrew, Snap, Nix, and on Windows the official installer, winget and
  Scoop; not Chocolatey), runs the command above and parses the reply.
- `src-tauri/src/token.rs`: the index gains a flag for a glab link, which is
  kept only with a login and an instance. The in-memory cache records the
  token, when it was checked and its expiry. Saving a token or clearing the
  slot removes the link.
- Commands `glab_status` (where glab was found) and `token_link_glab` (link
  the default GitLab slot to glab's login on an instance, gitlab.com when
  none is given). Requests that arrive together reuse one glab read. If that read fails, the requests already waiting return the same error instead of running glab again.
- UI: **Use GitLab CLI login**, setup steps (`GlabSetup.tsx`), "from GitLab
  CLI" and **Unlink GitLab CLI** in Settings, and the CLI-first order on
  both tabs of the setup screen.
- Docs: `SECURITY.md`, `docs/tokens.md`, `README.md`,
  `docs/development.md`.

### Not done yet

- Manual testing on macOS (opened from Finder), Windows and a Linux desktop.
  Tests here use fake `glab` scripts.
