# Access tokens

IsItGreen reads GitHub and GitLab with the login of [GitHub CLI](#github-cli)
or [GitLab CLI](#gitlab-cli), or with personal access tokens that you create.
The setup screen lists the CLI login first and the token form below it. Tokens
are stored in your operating system's keychain and never shown again after you
save them. All requests are read-only; see
[SECURITY.md](../SECURITY.md). You can connect either service or both; the
rest of this page covers GitHub first, then [GitLab](#gitlab).

## Fine-grained token (recommended)

A fine-grained token needs *read-only* access to: Metadata, Contents, Pull requests, Issues, Commit statuses and
Actions. [This link](https://github.com/settings/personal-access-tokens/new?name=IsItGreen&description=Read-only+access+for+the+IsItGreen+dashboard&metadata=read&contents=read&pull_requests=read&issues=read&statuses=read&actions=read)
opens GitHub's token form with those permissions filled in; the setup screen
links to it too. You still choose *Repository access* yourself (*All
repositories* or a selection), because GitHub cannot fill that in from a link.
Repository access alone grants no permissions, so a token with *All
repositories* but no permissions cannot read anything. A fine-grained token is
limited to one account or organization.

## Classic token

Works across organizations. Public repositories need no scopes; private repositories need `repo`, which also grants write access.
IsItGreen only sends read-only queries, but prefer fine-grained tokens where
possible.

## Organizations with SAML SSO

A classic token must be authorized for each organization that enforces SSO: on <https://github.com/settings/tokens>, choose
*Configure SSO* next to the token and authorize the organization. You can do
this yourself; no admin is needed. An OAuth token (`gho_`, for example from
`gh auth token`) does not appear on that page: sign in through the
organization's SSO and authorize the app again instead. Until then, the
organization's repositories show an SSO error and the "Needs you" list notes
that some results are hidden.

## Organization token policies

An organization can require owner approval for fine-grained tokens, or forbid classic or long-lived tokens. While a
fine-grained token is pending approval, or when a repository was not selected
for it, GitHub reports the repository as not found. If the organization has
approved GitHub CLI, its login usually still works; see below.

## GitHub CLI

If [GitHub CLI](https://cli.github.com) (`gh`) is installed and signed in
(`gh auth login`), the setup screen offers **Use GitHub CLI login**. IsItGreen
then uses the account gh is signed in to:

- **No copy is stored.** IsItGreen records only which account it is linked to,
  and asks gh for the token when it needs one (`gh auth token`), keeping it in
  memory only. `gh auth refresh` and signing in again are picked up on their
  own; `gh auth logout` disconnects IsItGreen too, within a minute (IsItGreen asks
  gh again at least once a minute, since signing out does not revoke the token).
- **Organizations that block personal access tokens.** gh's token is an OAuth
  token. It can read an organization's private repositories when the
  organization has approved GitHub CLI as an OAuth app, which many have, even
  where fine-grained tokens need approval or are blocked. If the organization
  has not approved it, only public data is visible. For SAML SSO, sign in
  through the organization's SSO and authorize GitHub CLI again.
- **It can also make changes.** gh always asks GitHub for at least the `repo`,
  `read:org` and `gist` scopes, and GitHub has no read-only scope for private
  repositories. IsItGreen itself only ever runs its own read-only queries
  ([SECURITY.md](../SECURITY.md)), but prefer a fine-grained token where your
  organization allows one.
- **Pinned to one account.** IsItGreen links to the account gh is using at that
  moment and keeps using that account, even after `gh auth switch`. To link
  another account, switch gh to it first, then link again. In **Settings →
  Access tokens → Per-owner tokens**, enter an owner and choose **Use GitHub
  CLI** to use gh's login for just that owner.
- **Where gh must be installed.** IsItGreen only runs gh from its usual install
  locations, never from `PATH`; see [Setting up GitHub CLI](#setting-up-github-cli).
- IsItGreen never uses `GH_TOKEN` or `GITHUB_TOKEN`, even if they are set; only
  tokens gh itself stores are used. gh 2.40 or newer is needed.

### Setting up GitHub CLI

If the setup screen does not offer **Use GitHub CLI login**, open *Use your
GitHub CLI login instead* there (or in Settings under per-owner tokens) for
these steps with the exact locations for your system:

1. Install GitHub CLI: `brew install gh` on macOS, `winget install --id
   GitHub.cli` on Windows, or see the
   [install guide](https://github.com/cli/cli#installation).
2. In a terminal, run `gh auth login` and choose *GitHub.com*. With several
   accounts, `gh auth switch` picks the one IsItGreen should use.
3. Make sure gh is in one of the places IsItGreen looks:

   | System | Locations |
   | --- | --- |
   | macOS | `/opt/homebrew/bin`, `/usr/local/bin`, `/opt/local/bin` (MacPorts) |
   | Linux | `/usr/bin`, `/usr/local/bin`, `/home/linuxbrew/.linuxbrew/bin`, `/snap/bin`, `/run/current-system/sw/bin` (NixOS) |
   | macOS and Linux | `~/.nix-profile/bin`, `/nix/var/nix/profiles/default/bin` (Nix) |
   | Windows | `%ProgramFiles%\GitHub CLI` (official installer), `%LOCALAPPDATA%\Microsoft\WinGet\Links`, `%USERPROFILE%\scoop\shims` (Chocolatey's folder is left out: other users of the PC could create it) |

   If `which gh` shows somewhere else (for example `~/.local/bin` or a
   version manager), link it into one of them:

   ```sh
   sudo mkdir -p /usr/local/bin && sudo ln -sf "$(which gh)" /usr/local/bin/gh
   ```

   On Windows, if `where gh` shows another location, install gh again with
   one of the installers above. IsItGreen refuses a gh that anyone could
   replace (one in, or linked from, a folder everyone can write to).
4. Choose **Check again**, then **Use GitHub CLI login**.

If IsItGreen then says GitHub CLI is not signed in, run `gh auth login` again
(or `gh auth status` to see which accounts gh knows). Once fixed in gh, the
next refresh works without linking again.

## Several accounts or organizations

IsItGreen keeps a **default token** plus optional **per-owner tokens**, so you
can use a fine-grained token for your personal repositories and another for
each organization:

1. Add the default token on first launch (usually one for your own account).
2. In **Settings → Access tokens → Per-owner tokens**, enter an account or
   organization name and a token for it. IsItGreen verifies the token with
   GitHub and shows the login it belongs to; the token itself is never shown
   again.
3. Pinned repositories owned by that account or organization are fetched with
   its token. All other repositories use the default token.

The **"Needs you"** sidebar searches with the default token, then merges in
results from per-owner tokens (or GitHub CLI links) that belong to the same
GitHub user,
de-duplicated. Items visible only to a token that belongs to a *different*
user are not shown, and per-owner tokens that fail are skipped there (their
repository cards show the error instead).

## GitLab

IsItGreen works with gitlab.com and with self-hosted GitLab 18 or newer that is
reachable over HTTPS. Certificates from a private certificate authority work
when your operating system trusts that authority. Choose **GitLab** on the setup screen, or **Settings →
Access tokens → Connect GitLab** once GitHub is connected.

1. Enter your instance, such as `gitlab.com` or `gitlab.example.com` (add the
   port if it is not 443).
2. Create a personal access token with only the **`read_api`** scope. On
   gitlab.com the setup screen links to the token form with the name and
   scope filled in; on a self-hosted instance it shows the address of that
   form (*User settings → Access tokens*).
3. Paste the token. IsItGreen checks it with that instance and shows the
   username it belongs to.

To use your GitLab CLI login instead, see [GitLab CLI](#gitlab-cli).

The instance is saved with the token, and the token is only ever sent to that
instance. One token covers every group and project your account can see, so
GitLab has no per-owner tokens. One GitLab instance can be connected at a
time; connecting another replaces it.

Pin a project in Settings with `group/project`, `group/subgroup/project` or a
link to any page of it; when both services are connected, pick the service
next to the field (a pasted link picks it for you). Merge requests fill the
pull request lists, the card's CI shows the latest pipeline on the default
branch, and the **"Needs you"** sidebar adds merge requests waiting on your
review, your own merge requests, and issues and merge requests assigned to
you.

Approval rules beyond what GitLab Free reports are not shown: a merge request
counts as approved once someone has approved it and GitLab reports no
missing approvals.

## GitLab CLI

If [GitLab CLI](https://gitlab.com/gitlab-org/cli) (`glab`) is installed and
signed in (`glab auth login`), the GitLab tab of the setup screen offers **Use
GitLab CLI login**. It sits above the token form, and it uses the instance in
the host field above it. IsItGreen then uses the account glab is signed in to
on that instance:

- **No copy is stored.** IsItGreen records only the instance and the account
  it is linked to. It asks glab for the token when it needs one (`glab auth
  credential-helper`), and keeps it in memory only. If glab's login is an OAuth
  login that has expired, glab refreshes it during that call and saves the new
  refresh token in its own config.
- **Sign-out and revocation.** IsItGreen asks glab again at least once a
  minute, and sooner when glab reports that the token is about to expire. If
  you sign out of glab (`glab auth logout`), IsItGreen disconnects within a
  minute. This holds even when a token was set for glab without an instance
  (`glab config set token --global`): IsItGreen refuses such a token, so it
  never links to one. A token revoked on GitLab is a different case. glab
  still returns it, so the recheck does not notice. The next request fails,
  because GitLab answers a rejected token as an anonymous request, and
  IsItGreen shows the setup screen so you can link again.
- **It can also make changes.** glab's token carries the `api` scope, which
  can write. IsItGreen itself only ever runs its own read-only queries
  ([SECURITY.md](../SECURITY.md)), but prefer a `read_api` token if you want
  the token itself to be read-only.
- **Pinned to one account and instance.** IsItGreen links to the account glab
  has on the chosen instance at that moment. If glab later returns a token for
  another account, IsItGreen refuses it and asks you to link again. Linking
  replaces any token you saved for GitLab, and saving a token or linking
  another instance replaces the link. As with a token, one GitLab instance is
  connected at a time, and there are no per-owner links.
- **Only login tokens.** IsItGreen accepts glab's personal access token and
  OAuth logins. It refuses a CI job token, and a token that glab holds for no
  particular instance. If that is the only token glab has, linking says GitLab
  CLI is not signed in to this instance; run `glab auth login --hostname
  <your instance>`.
- **Some glab setups cannot be linked.** If glab's entry for your instance
  uses `api_host`, or the instance is installed under a subfolder, glab
  reports the API host or the path as the instance. IsItGreen refuses that
  and shows "Could not get a token from GitLab CLI". Use a personal access
  token instead.
- **Where glab must be installed.** IsItGreen only runs glab from its usual
  install locations, never from `PATH`; see
  [Setting up GitLab CLI](#setting-up-gitlab-cli).
- IsItGreen ignores `GITLAB_TOKEN`, `GITLAB_ACCESS_TOKEN`, `GLAB_CONFIG_DIR`
  and `GITLAB_HOST`, even if they are set. It also ignores any glab
  configuration inside the repository it was started from. Only the login in
  glab's own user configuration is used. glab 1.85 or newer is needed.
- **Proxies and certificates.** IsItGreen passes `HTTPS_PROXY`, `HTTP_PROXY`,
  `NO_PROXY`, `ALL_PROXY` (and their lowercase forms), `SSL_CERT_FILE` and
  `SSL_CERT_DIR` on to glab so that an OAuth refresh can reach your instance.
  Keep that in mind if those variables point at a proxy or a certificate
  authority that you do not trust.
- **A rare side effect.** IsItGreen stops glab after 20 seconds. If that
  happens in the middle of an OAuth refresh, glab can lose its new refresh
  token, and you must run `glab auth login` again.

### Setting up GitLab CLI

If the setup screen does not offer **Use GitLab CLI login**, open *How to set
up GitLab CLI* there for these steps with the exact locations for your system:

1. Install GitLab CLI: `brew install glab` on macOS, `winget install
   glab.glab` or `scoop install glab` on Windows, or a package or `sudo snap
   install glab` on Linux. See the
   [install guide](https://gitlab.com/gitlab-org/cli#installation). You need
   version 1.85 or newer.
2. In a terminal, run `glab auth login --hostname <your instance>`, for
   example `glab auth login --hostname gitlab.com`.
3. Make sure glab is in one of the places IsItGreen looks:

   | System | Locations |
   | --- | --- |
   | macOS | `/opt/homebrew/bin`, `/usr/local/bin`, `/opt/local/bin` (MacPorts) |
   | Linux | `/usr/bin`, `/usr/local/bin`, `/home/linuxbrew/.linuxbrew/bin`, `/snap/bin`, `/run/current-system/sw/bin` (NixOS) |
   | macOS and Linux | `~/.nix-profile/bin`, `/nix/var/nix/profiles/default/bin` (Nix) |
   | Windows | `%ProgramFiles%\glab` (official installer), `%LOCALAPPDATA%\Microsoft\WinGet\Links`, `%USERPROFILE%\scoop\shims` (Chocolatey's folder is left out: other users of the PC could create it) |

   If `which glab` shows somewhere else, link it into one of them:

   ```sh
   sudo mkdir -p /usr/local/bin && sudo ln -sf "$(which glab)" /usr/local/bin/glab
   ```

   On Windows, if `where glab` shows another location, install glab again with
   one of the installers above. IsItGreen refuses a glab that anyone could
   replace (one in, or linked from, a folder everyone can write to).
4. Choose **Check again**, then **Use GitLab CLI login**.

If IsItGreen then says GitLab CLI is not signed in to the instance, run `glab
auth login --hostname <your instance>` again. Once fixed in glab, the next
refresh works without linking again. In Settings, the linked connection shows
*from GitLab CLI* and **Unlink GitLab CLI** removes the link.
