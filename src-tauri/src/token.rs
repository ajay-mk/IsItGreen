//! Access-token storage backed by the operating system's keychain.
//!
//! Tokens never leave the Rust side of the app: the frontend can ask which
//! tokens exist, set a new one, or clear one, but it can never read one back.
//!
//! Each provider has a *default* token plus optional *per-owner* tokens keyed
//! by account/organization login. Fine-grained GitHub tokens are limited to a
//! single resource owner, so per-owner tokens let one app cover a personal
//! account and several organizations at once.
//!
//! A GitHub slot can also be *linked to the GitHub CLI*: then no token is
//! stored at all. The index records the login the link is pinned to, and the
//! token is read from `gh` when needed and kept in memory only (`gh.rs`).
//! The default GitLab slot can be linked to the GitLab CLI the same way: the
//! index records the instance and login, and the token is read from `glab`
//! (`glab.rs`).

use std::collections::{hash_map, BTreeMap, BTreeSet, HashMap};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::gitlab::{is_valid_login, Host};
use crate::glab::GlabToken;

const SERVICE: &str = "io.github.ajay-mk.isitgreen";
const MAX_TOKEN_LEN: usize = 255;
const MIN_TOKEN_LEN: usize = 20;
const MAX_OWNER_LEN: usize = 39;
/// Upper bound on per-owner tokens, which also bounds the index entry size.
pub const MAX_OWNER_TOKENS: usize = 20;
/// How long a token read from the GitHub or GitLab CLI is used before the
/// CLI is asked again. `gh auth logout` does not revoke the token, so this is
/// what makes signing out of gh disconnect IsItGreen too.
pub const CLI_RECHECK: Duration = Duration::from_secs(60);
/// A token read from the GitLab CLI that expires within this margin is not
/// used again: glab is asked for a refreshed one instead.
pub const GLAB_EXPIRY_MARGIN: Duration = Duration::from_secs(30);

/// Providers that can hold a token. Kept as an allow-list so the frontend
/// cannot create arbitrary keychain entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provider {
    GitHub,
    GitLab,
}

impl Provider {
    pub fn parse(id: &str) -> Result<Self> {
        match id {
            "github" => Ok(Provider::GitHub),
            "gitlab" => Ok(Provider::GitLab),
            other => Err(Error::UnknownProvider(other.chars().take(32).collect())),
        }
    }

    fn account(self) -> &'static str {
        match self {
            Provider::GitHub => "github",
            Provider::GitLab => "gitlab",
        }
    }
}

/// A validated account or organization login, normalised to lowercase
/// (GitHub logins are case-insensitive).
///
/// Uses the same rules as `OWNER_RE` in `src/config/schema.ts`: 1-39 ASCII
/// letters, digits or hyphens, starting with a letter or digit. This keeps
/// keychain entry names to a fixed, predictable shape.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Owner(String);

impl Owner {
    pub fn parse(login: &str) -> Result<Self> {
        let bytes = login.as_bytes();
        let valid = (1..=MAX_OWNER_LEN).contains(&bytes.len())
            && bytes[0].is_ascii_alphanumeric()
            && bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'-');
        if valid {
            Ok(Owner(login.to_ascii_lowercase()))
        } else {
            Err(Error::InvalidOwner)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which token of a provider: the default one or the one for an owner.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Default,
    Owner(Owner),
}

impl Slot {
    pub fn from_owner(owner: Option<Owner>) -> Self {
        owner.map_or(Slot::Default, Slot::Owner)
    }

    /// Keychain account name. The default slot keeps the pre-multi-token name
    /// so existing installs keep working. Owners cannot contain ':', so owner
    /// entries can never collide with each other or with the index entry.
    fn account(&self, provider: Provider) -> String {
        match self {
            Slot::Default => provider.account().to_owned(),
            Slot::Owner(owner) => format!("{}:owner:{}", provider.account(), owner.as_str()),
        }
    }
}

fn index_account(provider: Provider) -> String {
    format!("{}:index", provider.account())
}

/// Basic sanity check before a token is sent anywhere or stored.
///
/// GitHub tokens (classic `ghp_`, fine-grained `github_pat_`, OAuth `gho_`, ...)
/// only contain ASCII letters, digits and underscores. GitLab tokens
/// (`glpat-...`, newer ones with `.`-separated parts) also use `-` and `.`,
/// and a self-hosted instance's custom token prefix may add `+=/@:`.
pub fn validate_format(provider: Provider, token: &str) -> Result<()> {
    let len = token.len();
    let charset_ok = token.bytes().all(|b| {
        b.is_ascii_alphanumeric()
            || b == b'_'
            || (provider == Provider::GitLab && b"-.+=/@:".contains(&b))
    });
    if (MIN_TOKEN_LEN..=MAX_TOKEN_LEN).contains(&len) && charset_ok {
        Ok(())
    } else {
        Err(Error::InvalidTokenFormat)
    }
}

/// Non-secret bookkeeping stored next to the tokens: which owners have a
/// token, the login each token belongs to (the keychain cannot be
/// enumerated portably), and for GitLab the instance the default token is for.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Index {
    #[serde(default)]
    default_login: Option<String>,
    /// GitLab only: the instance the default token was verified against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    /// Owner -> login of the account the token belongs to.
    #[serde(default)]
    owners: BTreeMap<String, String>,
    /// GitHub only: the default slot is linked to the GitHub CLI account
    /// `default_login` instead of holding a stored token.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    default_gh: bool,
    /// GitHub only: owners whose slot is linked to the GitHub CLI account
    /// recorded for them in `owners`.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    gh_owners: BTreeSet<String>,
    /// GitLab only: the default slot is linked to the GitLab CLI account
    /// `default_login` on `host` instead of holding a stored token.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    default_glab: bool,
}

impl Index {
    /// Parses a stored index, dropping anything malformed instead of failing,
    /// so a damaged entry can never lock the user out.
    fn decode(raw: &str) -> Self {
        let Ok(index) = serde_json::from_str::<Index>(raw) else {
            return Index::default();
        };
        let valid_login = |l: &String| is_valid_login(l);
        let default_login = index.default_login.filter(valid_login);
        let owners: BTreeMap<String, String> = index
            .owners
            .into_iter()
            .filter(|(owner, login)| {
                Owner::parse(owner).is_ok_and(|o| o.as_str() == owner) && valid_login(login)
            })
            .take(MAX_OWNER_TOKENS)
            .collect();
        let had_host = index.host.is_some();
        let host = index
            .host
            .filter(|h| Host::parse(h).is_ok_and(|p| p.as_str() == h));
        Index {
            default_gh: index.default_gh && default_login.is_some() && !had_host,
            gh_owners: index
                .gh_owners
                .into_iter()
                .filter(|owner| owners.contains_key(owner))
                .collect(),
            // A glab link is pinned to an instance, so unlike a gh link it
            // needs the host.
            default_glab: index.default_glab && default_login.is_some() && host.is_some(),
            default_login,
            host,
            owners,
        }
    }

    /// The instance and login the default slot is linked to through the
    /// GitLab CLI, if it is.
    fn glab_link(&self) -> Option<(Host, &str)> {
        if !self.default_glab {
            return None;
        }
        let host = Host::parse(self.host.as_deref()?).ok()?;
        Some((host, self.default_login.as_deref()?))
    }

    /// The login a slot is linked to through the GitHub CLI, if it is.
    fn gh_login(&self, slot: &Slot) -> Option<&str> {
        match slot {
            Slot::Default => self.default_login.as_deref().filter(|_| self.default_gh),
            Slot::Owner(owner) => self
                .owners
                .get(owner.as_str())
                .map(String::as_str)
                .filter(|_| self.gh_owners.contains(owner.as_str())),
        }
    }

    fn source(&self, slot: &Slot) -> Source {
        if self.gh_login(slot).is_some() {
            Source::GitHubCli
        } else if *slot == Slot::Default && self.glab_link().is_some() {
            Source::GitLabCli
        } else {
            Source::Token
        }
    }

    fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }
}

/// Where a slot's token comes from.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// A token stored in the keychain.
    Token,
    /// Read from the GitHub CLI when needed, never stored.
    #[serde(rename = "gh")]
    GitHubCli,
    /// Read from the GitLab CLI when needed, never stored.
    #[serde(rename = "glab")]
    GitLabCli,
}

/// What the frontend may learn about the stored tokens. Never the tokens.
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenSummary {
    pub has_default: bool,
    pub default_login: Option<String>,
    /// Where the default token comes from, when there is one.
    pub default_source: Option<Source>,
    /// GitLab only: the instance the default token is for.
    pub host: Option<String>,
    pub owners: Vec<OwnerToken>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct OwnerToken {
    pub owner: String,
    pub login: String,
    pub source: Source,
}

/// The token picked for a request.
pub enum Resolved {
    /// A stored token, or one read from the GitHub CLI earlier and checked.
    Token { slot: Slot, token: String },
    /// The slot is linked to the GitHub CLI account `login` and no checked
    /// token is in memory: the caller must read it from gh, check it belongs
    /// to `login`, and hand it to [`TokenStore::remember_gh`].
    FromGh { slot: Slot, login: String },
}

/// The GitLab token picked for a request, with the instance it is sent to.
pub enum ResolvedGitLab {
    /// A stored token, or one read from the GitLab CLI earlier and checked.
    Token {
        slot: Slot,
        token: String,
        host: Host,
    },
    /// The default slot is linked to the GitLab CLI account `login` on
    /// `host` and no fresh token is in memory: the caller must read it from
    /// glab, check it belongs to `login`, and hand it to
    /// [`TokenStore::remember_glab`].
    FromGlab { host: Host, login: String },
}

/// A token read from the GitLab CLI, kept in memory only.
struct GlabCached {
    token: String,
    /// When glab last gave this token.
    checked_at: Instant,
    /// When glab says the token expires (OAuth tokens only).
    expires: Option<SystemTime>,
}

impl GlabCached {
    /// Whether the token may be used without asking glab again.
    fn fresh(&self) -> bool {
        let lasts = self.expires.is_none_or(|at| {
            at.duration_since(SystemTime::now())
                .is_ok_and(|left| left > GLAB_EXPIRY_MARGIN)
        });
        self.checked_at.elapsed() < CLI_RECHECK && lasts
    }
}

#[derive(Default)]
struct Inner {
    cache: HashMap<(Provider, Slot), String>,
    indexes: HashMap<Provider, Index>,
    /// When gh last confirmed the cached token of a linked slot.
    gh_checked: HashMap<(Provider, Slot), Instant>,
    /// The token of the default GitLab slot when it is linked to glab. Kept
    /// apart from `cache`, so it is never taken for a stored token.
    glab: Option<GlabCached>,
}

impl Inner {
    /// The cached token of a slot linked to `gh`, and whether gh confirmed
    /// it recently enough to use without asking again.
    fn gh_cached(&self, provider: Provider, slot: &Slot) -> Option<(String, bool)> {
        let key = (provider, slot.clone());
        let token = self.cache.get(&key)?.clone();
        let fresh = self
            .gh_checked
            .get(&key)
            .is_some_and(|at| at.elapsed() < CLI_RECHECK);
        Some((token, fresh))
    }

    /// The cached glab token and whether it is fresh, if the default GitLab
    /// slot is still linked to `login` on `host`. The cache is only filled
    /// for the current link and dropped whenever the link changes, so the
    /// token was read for that same instance and account.
    fn glab_cached(&mut self, host: &Host, login: &str) -> Option<(String, bool)> {
        let linked = self
            .index(Provider::GitLab)
            .ok()?
            .glab_link()
            .is_some_and(|(h, l)| h == *host && l == login);
        let cached = self.glab.as_ref()?;
        linked.then(|| (cached.token.clone(), cached.fresh()))
    }
}

/// In-memory cache in front of the keychain, so we do not hit the keychain
/// (which may prompt the user on some platforms) for every request.
///
/// Deliberately does not implement `Debug` so tokens cannot end up in logs.
#[derive(Default)]
pub struct TokenStore {
    inner: Mutex<Inner>,
}

fn entry(account: &str) -> Result<keyring::Entry> {
    Ok(keyring::Entry::new(SERVICE, account)?)
}

fn read(account: &str) -> Result<String> {
    Ok(entry(account)?.get_password()?)
}

fn delete(entry: &keyring::Entry) -> Result<()> {
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn delete_account(account: &str) -> Result<()> {
    delete(&entry(account)?)
}

impl Inner {
    fn index(&mut self, provider: Provider) -> Result<&mut Index> {
        match self.indexes.entry(provider) {
            hash_map::Entry::Occupied(e) => Ok(e.into_mut()),
            hash_map::Entry::Vacant(e) => {
                let index = match read(&index_account(provider)) {
                    Ok(raw) => Index::decode(&raw),
                    Err(Error::NoToken) => Index::default(),
                    Err(err) => return Err(err),
                };
                Ok(e.insert(index))
            }
        }
    }

    fn save_index(&mut self, provider: Provider) -> Result<()> {
        let raw = self.index(provider)?.encode();
        entry(&index_account(provider))?.set_password(&raw)?;
        Ok(())
    }

    fn get(&mut self, provider: Provider, slot: &Slot) -> Result<String> {
        let key = (provider, slot.clone());
        if let Some(token) = self.cache.get(&key) {
            return Ok(token.clone());
        }
        let token = read(&slot.account(provider))?;
        self.cache.insert(key, token.clone());
        Ok(token)
    }
}

impl TokenStore {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner>> {
        self.inner.lock().map_err(|_| Error::Internal)
    }

    /// Picks the token that serves `owner`: that owner's own token if one is
    /// configured, otherwise the default token. Returns the slot used, so the
    /// caller can forget it if the provider rejects it.
    ///
    /// A slot linked to the GitHub CLI is served from memory, or else by
    /// running `gh`; such a fresh token comes back with `verify_login` set.
    /// Blocking (keychain, and possibly `gh`): call from a blocking-safe
    /// context.
    pub fn resolve(&self, provider: Provider, owner: Option<&Owner>) -> Result<Resolved> {
        let mut inner = self.lock()?;
        let mut slot = match owner {
            Some(owner) if inner.index(provider)?.owners.contains_key(owner.as_str()) => {
                Slot::Owner(owner.clone())
            }
            _ => Slot::Default,
        };
        loop {
            let linked = inner.index(provider)?.gh_login(&slot).map(str::to_owned);
            if let Some(login) = linked {
                return Ok(match inner.gh_cached(provider, &slot) {
                    Some((token, true)) => Resolved::Token { token, slot },
                    _ => Resolved::FromGh { slot, login },
                });
            }
            match inner.get(provider, &slot) {
                Ok(token) => return Ok(Resolved::Token { slot, token }),
                // An owner's token removed from the keychain behind our
                // back: fall back to the default one.
                Err(Error::NoToken) if slot != Slot::Default => slot = Slot::Default,
                Err(err) => return Err(err),
            }
        }
    }

    /// The GitHub CLI token for `slot` already in memory, if the slot is still
    /// linked to `login`, and whether gh confirmed it within [`CLI_RECHECK`].
    pub fn cached_gh(
        &self,
        provider: Provider,
        slot: &Slot,
        login: &str,
    ) -> Option<(String, bool)> {
        let mut inner = self.lock().ok()?;
        let linked = inner
            .index(provider)
            .is_ok_and(|index| index.gh_login(slot) == Some(login));
        if !linked {
            return None;
        }
        inner.gh_cached(provider, slot)
    }

    /// Keeps a token read from the GitHub CLI in memory once the caller has
    /// checked it belongs to `login`, unless the slot was unlinked or linked
    /// to another account in the meantime.
    pub fn remember_gh(&self, provider: Provider, slot: &Slot, login: &str, token: &str) {
        if let Ok(mut inner) = self.lock() {
            let still_linked = inner
                .index(provider)
                .is_ok_and(|index| index.gh_login(slot) == Some(login));
            if still_linked {
                let key = (provider, slot.clone());
                inner.cache.insert(key.clone(), token.to_owned());
                inner.gh_checked.insert(key, Instant::now());
            }
        }
    }

    /// Links `slot` to the GitHub CLI account `login`, whose `token` the
    /// caller has just verified. Any token stored for the slot is deleted, so
    /// no copy is left behind; from now on the token is only read from `gh`.
    pub fn link_gh(&self, provider: Provider, slot: &Slot, login: &str, token: &str) -> Result<()> {
        if provider != Provider::GitHub {
            return Err(Error::OwnerTokensUnsupported);
        }
        if !is_valid_login(login) {
            return Err(Error::InvalidOwner);
        }
        let mut inner = self.lock()?;
        if let Slot::Owner(owner) = slot {
            let owners = &inner.index(provider)?.owners;
            if !owners.contains_key(owner.as_str()) && owners.len() >= MAX_OWNER_TOKENS {
                return Err(Error::TooManyTokens);
            }
        }
        let previous = inner.index(provider)?.clone();
        let index = inner.index(provider)?;
        match slot {
            Slot::Default => {
                index.default_login = Some(login.to_owned());
                index.default_gh = true;
            }
            Slot::Owner(owner) => {
                index
                    .owners
                    .insert(owner.as_str().to_owned(), login.to_owned());
                index.gh_owners.insert(owner.as_str().to_owned());
            }
        }
        if let Err(err) = inner.save_index(provider) {
            // Nothing was deleted yet, so the slot is exactly as it was.
            *inner.index(provider)? = previous;
            return Err(err);
        }
        let key = (provider, slot.clone());
        inner.cache.insert(key.clone(), token.to_owned());
        inner.gh_checked.insert(key.clone(), Instant::now());
        // Only now that the link is saved, drop the stored token, so a failed
        // save can never lose it.
        let account = slot.account(provider);
        if let Err(err) = delete_account(&account) {
            // The stored token is still there: go back to it, so the error
            // matches what is stored.
            let linked = std::mem::replace(inner.index(provider)?, previous);
            if inner.save_index(provider).is_err() {
                // The keychain still holds the link, so keep it in memory
                // too, or it would come back after a restart. The old token
                // left behind is never read for a linked slot, and clear()
                // deletes it.
                *inner.index(provider)? = linked;
                return Ok(());
            }
            inner.cache.remove(&key);
            inner.gh_checked.remove(&key);
            return Err(err);
        }
        Ok(())
    }

    /// The GitLab CLI token already in memory, if the default GitLab slot is
    /// still linked to `login` on `host`, and whether it is fresh: read within
    /// [`CLI_RECHECK`] and not about to expire.
    pub fn cached_glab(&self, host: &Host, login: &str) -> Option<(String, bool)> {
        self.lock().ok()?.glab_cached(host, login)
    }

    /// Keeps a token read from the GitLab CLI in memory once the caller has
    /// checked it belongs to `login` on `host`, unless the slot was unlinked
    /// or linked to another instance or account in the meantime. Returns
    /// whether it is still linked; if not, the token must not be sent.
    pub fn remember_glab(&self, host: &Host, login: &str, token: &GlabToken) -> bool {
        let Ok(mut inner) = self.lock() else {
            return false;
        };
        let still_linked = inner.index(Provider::GitLab).is_ok_and(|index| {
            index
                .glab_link()
                .is_some_and(|(h, l)| h == *host && l == login)
        });
        if still_linked {
            inner.glab = Some(GlabCached {
                token: token.token.clone(),
                checked_at: token.read_at,
                expires: token.expires,
            });
        }
        still_linked
    }

    /// Links the default GitLab slot to the GitLab CLI account `login` on
    /// `host`, whose `token` the caller has just verified. Any stored default
    /// GitLab token is deleted; from now on the token is only read from glab.
    pub fn link_glab(&self, host: &Host, login: &str, token: &GlabToken) -> Result<()> {
        if !is_valid_login(login) {
            return Err(Error::InvalidOwner);
        }
        let provider = Provider::GitLab;
        let mut inner = self.lock()?;
        let previous = inner.index(provider)?.clone();
        let index = inner.index(provider)?;
        index.default_login = Some(login.to_owned());
        index.host = Some(host.as_str().to_owned());
        index.default_glab = true;
        if let Err(err) = inner.save_index(provider) {
            // Nothing was deleted yet, so the slot is exactly as it was.
            *inner.index(provider)? = previous;
            return Err(err);
        }
        inner.glab = Some(GlabCached {
            token: token.token.clone(),
            checked_at: token.read_at,
            expires: token.expires,
        });
        inner.cache.remove(&(provider, Slot::Default));
        // As in link_gh: delete the stored token only once the link is saved,
        // and go back to it if that fails.
        if let Err(err) = delete_account(&Slot::Default.account(provider)) {
            let linked = std::mem::replace(inner.index(provider)?, previous);
            if inner.save_index(provider).is_err() {
                // As in link_gh: the keychain still holds the link, so memory
                // keeps it too.
                *inner.index(provider)? = linked;
                return Ok(());
            }
            inner.glab = None;
            return Err(err);
        }
        Ok(())
    }

    pub fn summary(&self, provider: Provider) -> Result<TokenSummary> {
        let mut inner = self.lock()?;
        let index = inner.index(provider)?;
        let linked = index.gh_login(&Slot::Default).is_some()
            || (provider == Provider::GitLab && index.glab_link().is_some());
        let stored = linked
            || match inner.get(provider, &Slot::Default) {
                Ok(_) => true,
                Err(Error::NoToken) => false,
                Err(err) => return Err(err),
            };
        let index = inner.index(provider)?;
        // A GitLab token is only usable with the instance it was verified
        // against; one left without it by an interrupted write must be
        // entered again, host included.
        let has_default = stored && (provider != Provider::GitLab || index.host.is_some());
        Ok(TokenSummary {
            has_default,
            default_login: index.default_login.clone().filter(|_| has_default),
            default_source: has_default.then(|| index.source(&Slot::Default)),
            host: index.host.clone().filter(|_| has_default),
            owners: index
                .owners
                .iter()
                .map(|(owner, login)| OwnerToken {
                    owner: owner.clone(),
                    login: login.clone(),
                    source: index.source(&Slot::Owner(Owner(owner.clone()))),
                })
                .collect(),
        })
    }

    /// The default token together with the instance it was verified against,
    /// read under one lock so a concurrent `set` cannot pair a token with
    /// another token's host. A slot linked to the GitLab CLI is served from
    /// memory while its token is fresh; otherwise the caller is told to ask
    /// glab. Blocking: call from a blocking-safe context.
    pub fn resolve_with_host(&self, provider: Provider) -> Result<ResolvedGitLab> {
        let mut inner = self.lock()?;
        let index = inner.index(provider)?;
        if provider == Provider::GitLab {
            if let Some((host, login)) = index.glab_link() {
                let login = login.to_owned();
                return Ok(match inner.glab_cached(&host, &login) {
                    Some((token, true)) => ResolvedGitLab::Token {
                        slot: Slot::Default,
                        token,
                        host,
                    },
                    _ => ResolvedGitLab::FromGlab { host, login },
                });
            }
        }
        let host = index
            .host
            .as_deref()
            .and_then(|h| Host::parse(h).ok())
            .ok_or(Error::NoToken)?;
        let token = inner.get(provider, &Slot::Default)?;
        Ok(ResolvedGitLab::Token {
            slot: Slot::Default,
            token,
            host,
        })
    }

    /// The GitLab instance the default token was verified against.
    pub fn host(&self, provider: Provider) -> Result<Option<Host>> {
        let mut inner = self.lock()?;
        Ok(inner
            .index(provider)?
            .host
            .as_deref()
            .and_then(|h| Host::parse(h).ok()))
    }

    /// Stores `token` (already verified to belong to `login`, on `host` for
    /// GitLab) in `slot`.
    pub fn set(
        &self,
        provider: Provider,
        slot: &Slot,
        token: &str,
        login: &str,
        host: Option<&Host>,
    ) -> Result<()> {
        validate_format(provider, token)?;
        let mut inner = self.lock()?;
        if let Slot::Owner(owner) = slot {
            let owners = &inner.index(provider)?.owners;
            if !owners.contains_key(owner.as_str()) && owners.len() >= MAX_OWNER_TOKENS {
                return Err(Error::TooManyTokens);
            }
        }
        // The stored host decides where the default token is sent, so a new
        // token must never be paired with an old host, even if a keychain
        // write fails halfway. Unbind the host first and bind it again only
        // once the token is stored: an interrupted change leaves no host,
        // which fails closed (nothing is sent) until the token is entered again.
        let previous = inner.index(provider)?.clone();
        let unbind = matches!(slot, Slot::Default) && previous.host.is_some();
        if unbind {
            let index = inner.index(provider)?;
            index.host = None;
            index.default_login = None;
            index.default_glab = false;
            if let Err(err) = inner.save_index(provider) {
                *inner.index(provider)? = previous;
                return Err(err);
            }
        }
        let stored = entry(&slot.account(provider)).and_then(|e| Ok(e.set_password(token)?));
        if let Err(err) = stored {
            if unbind {
                // The old token is still in place, so its host can be restored.
                *inner.index(provider)? = previous;
                let _ = inner.save_index(provider);
            }
            return Err(err);
        }
        inner
            .cache
            .insert((provider, slot.clone()), token.to_owned());
        let index = inner.index(provider)?;
        let unbound = index.clone();
        match slot {
            Slot::Default => {
                index.default_login = Some(login.to_owned());
                index.host = host.map(|h| h.as_str().to_owned());
                index.default_gh = false;
                index.default_glab = false;
            }
            Slot::Owner(owner) => {
                index
                    .owners
                    .insert(owner.as_str().to_owned(), login.to_owned());
                index.gh_owners.remove(owner.as_str());
            }
        }
        let saved = inner.save_index(provider);
        if provider == Provider::GitLab && matches!(slot, Slot::Default) {
            inner.glab = None;
        }
        if saved.is_err() && provider == Provider::GitLab && matches!(slot, Slot::Default) {
            // Keep memory in line with the stored index, and drop the token
            // so it is not left behind without the host it belongs to.
            *inner.index(provider)? = unbound;
            inner.cache.remove(&(provider, slot.clone()));
            if let Ok(e) = entry(&slot.account(provider)) {
                let _ = delete(&e);
            }
        }
        saved
    }

    pub fn clear(&self, provider: Provider, slot: &Slot) -> Result<()> {
        let mut inner = self.lock()?;
        inner.cache.remove(&(provider, slot.clone()));
        inner.gh_checked.remove(&(provider, slot.clone()));
        if provider == Provider::GitLab && *slot == Slot::Default {
            inner.glab = None;
        }
        let account = slot.account(provider);
        delete_account(&account)?;
        let previous = inner.index(provider)?.clone();
        let index = inner.index(provider)?;
        match slot {
            Slot::Default => {
                index.default_login = None;
                index.host = None;
                index.default_gh = false;
                index.default_glab = false;
            }
            Slot::Owner(owner) => {
                index.owners.remove(owner.as_str());
                index.gh_owners.remove(owner.as_str());
            }
        }
        if let Err(err) = inner.save_index(provider) {
            // Keep memory matching what is stored, so the slot still shows as
            // set (or linked to GitHub CLI) and removing it can be retried,
            // instead of a link that looks gone coming back after a restart.
            *inner.index(provider)? = previous;
            return Err(err);
        }
        Ok(())
    }

    /// Drops the cached copy (e.g. after the provider rejected it) without
    /// touching the keychain.
    pub fn forget_cached(&self, provider: Provider, slot: &Slot) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.cache.remove(&(provider, slot.clone()));
            inner.gh_checked.remove(&(provider, slot.clone()));
            if provider == Provider::GitLab && *slot == Slot::Default {
                inner.glab = None;
            }
        }
    }
}

/// A keychain for tests: in memory, kept across `keyring::Entry::new`, and
/// able to fail on demand. Each test runs on its own thread, so the entries
/// and failure switches are per thread and tests cannot see each other's.
#[cfg(test)]
pub(crate) mod test_keychain {
    use std::any::Any;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    use keyring::credential::{CredentialApi, CredentialBuilderApi};
    use keyring::{Credential, Error, Result};

    thread_local! {
        static ENTRIES: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
        static FAIL_DELETE: Cell<bool> = const { Cell::new(false) };
        static SETS_BEFORE_FAILING: Cell<Option<usize>> = const { Cell::new(None) };
    }

    /// Sends all keychain calls in this test binary here.
    pub fn install() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| keyring::set_default_credential_builder(Box::new(Builder)));
    }

    /// Makes every delete on this thread fail, or work again.
    pub fn fail_delete(fail: bool) {
        FAIL_DELETE.set(fail);
    }

    /// Lets `n` more writes on this thread succeed and fails the ones after;
    /// `None` lets all of them succeed.
    pub fn fail_set_after(n: Option<usize>) {
        SETS_BEFORE_FAILING.set(n);
    }

    /// What is stored for `account` on this thread.
    pub fn stored(account: &str) -> Option<String> {
        ENTRIES.with_borrow(|e| e.get(account).map(|v| String::from_utf8_lossy(v).into()))
    }

    fn failure() -> Error {
        Error::PlatformFailure("test keychain failure".into())
    }

    struct Builder;

    impl CredentialBuilderApi for Builder {
        fn build(&self, _: Option<&str>, _: &str, user: &str) -> Result<Box<Credential>> {
            Ok(Box::new(Entry(user.to_owned())))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[derive(Debug)]
    struct Entry(String);

    impl CredentialApi for Entry {
        fn set_secret(&self, secret: &[u8]) -> Result<()> {
            match SETS_BEFORE_FAILING.get() {
                Some(0) => return Err(failure()),
                Some(n) => SETS_BEFORE_FAILING.set(Some(n - 1)),
                None => {}
            }
            ENTRIES.with_borrow_mut(|e| e.insert(self.0.clone(), secret.to_vec()));
            Ok(())
        }

        fn get_secret(&self) -> Result<Vec<u8>> {
            ENTRIES.with_borrow(|e| e.get(&self.0).cloned().ok_or(Error::NoEntry))
        }

        fn delete_credential(&self) -> Result<()> {
            if FAIL_DELETE.get() {
                return Err(failure());
            }
            ENTRIES.with_borrow_mut(|e| e.remove(&self.0).map(|_| ()).ok_or(Error::NoEntry))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_github_token_shapes() {
        let gh = Provider::GitHub;
        assert!(validate_format(gh, "ghp_0123456789abcdefghijABCDEFGHIJ012345").is_ok());
        assert!(validate_format(gh, &format!("github_pat_{}", "a1".repeat(40))).is_ok());
        assert!(validate_format(gh, "glpat-0123456789abcdefghij").is_err());
    }

    #[test]
    fn accepts_gitlab_token_shapes() {
        let gl = Provider::GitLab;
        assert!(validate_format(gl, "glpat-0123456789abcdefghij").is_ok());
        assert!(validate_format(gl, "glpat-AbCdEf0123456789_xYz.01.0w1a2b3c4").is_ok());
        assert!(validate_format(gl, "glpat-0123456789 abcdefghij").is_err());
        assert!(validate_format(gl, "corp+pat=/@:-0123456789abcdefghij").is_ok());
        assert!(validate_format(gl, "glpat-0123456789\nabcdefghij").is_err());
        assert!(validate_format(gl, "glpat-0123456789%abcdefghij").is_err());
    }

    #[test]
    fn rejects_bad_tokens() {
        for provider in [Provider::GitHub, Provider::GitLab] {
            assert!(validate_format(provider, "").is_err());
            assert!(validate_format(provider, "short").is_err());
            assert!(validate_format(provider, "ghp_0123456789abcdef ghijABCDEFGHIJ").is_err());
            assert!(validate_format(provider, "ghp_0123456789abcdefghij\nABCDEFGHIJ").is_err());
            assert!(validate_format(provider, &"a".repeat(MAX_TOKEN_LEN + 1)).is_err());
        }
    }

    #[test]
    fn provider_allow_list() {
        assert_eq!(Provider::parse("github").unwrap(), Provider::GitHub);
        assert_eq!(Provider::parse("gitlab").unwrap(), Provider::GitLab);
        assert_eq!(Slot::Default.account(Provider::GitLab), "gitlab");
        assert_eq!(index_account(Provider::GitLab), "gitlab:index");
        assert!(Provider::parse("evil").is_err());
    }

    #[test]
    fn owner_validation_matches_frontend_rules() {
        for ok in ["a", "ajay-mk", "Acme-Corp", "0rg", &"a".repeat(39)] {
            assert!(Owner::parse(ok).is_ok(), "{ok} should be valid");
        }
        for bad in [
            "",
            "-acme",
            "acme_corp",
            "acme corp",
            "acme:index",
            "acme/repo",
            "../acme",
            "acmé",
            &"a".repeat(40),
        ] {
            assert!(
                matches!(Owner::parse(bad), Err(Error::InvalidOwner)),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn owner_is_case_insensitive() {
        assert_eq!(Owner::parse("Acme").unwrap(), Owner::parse("acme").unwrap());
        assert_eq!(Owner::parse("Acme").unwrap().as_str(), "acme");
    }

    #[test]
    fn keychain_account_names() {
        let gh = Provider::GitHub;
        // Unchanged from the single-token layout, so existing tokens still load.
        assert_eq!(Slot::Default.account(gh), "github");
        assert_eq!(
            Slot::Owner(Owner::parse("Acme-Corp").unwrap()).account(gh),
            "github:owner:acme-corp"
        );
        assert_eq!(index_account(gh), "github:index");
        assert_eq!(Slot::from_owner(None), Slot::Default);
    }

    #[test]
    fn owner_accounts_never_collide_with_reserved_names() {
        let gh = Provider::GitHub;
        for owner in ["index", "github", "owner"] {
            let account = Slot::Owner(Owner::parse(owner).unwrap()).account(gh);
            assert_ne!(account, Slot::Default.account(gh));
            assert_ne!(account, index_account(gh));
        }
    }

    #[test]
    fn index_round_trips() {
        let mut index = Index {
            default_login: Some("ajay-mk".into()),
            host: Some("gitlab.example.com:8443".into()),
            ..Index::default()
        };
        index.owners.insert("acme".into(), "ajay-mk".into());
        assert_eq!(Index::decode(&index.encode()), index);
    }

    #[test]
    fn index_decode_drops_malformed_entries() {
        let raw = r#"{
            "default_login": "bad login",
            "host": "https://evil.example.com/x",
            "owners": { "acme": "ajay-mk", "Upper": "x", "bad:owner": "x", "ok": "<script>" }
        }"#;
        let index = Index::decode(raw);
        assert_eq!(index.default_login, None);
        assert_eq!(index.host, None);
        assert_eq!(index.owners.len(), 1);
        assert_eq!(
            index.owners.get("acme").map(String::as_str),
            Some("ajay-mk")
        );

        assert_eq!(Index::decode("not json"), Index::default());
        assert_eq!(Index::decode("[]"), Index::default());
    }

    fn store_with(provider: Provider, host: Option<&str>) -> TokenStore {
        let store = TokenStore::default();
        {
            let mut inner = store.lock().unwrap();
            inner
                .cache
                .insert((provider, Slot::Default), "glpat-token".into());
            let index = Index {
                default_login: Some("jane.doe".into()),
                host: host.map(str::to_owned),
                ..Index::default()
            };
            inner.indexes.insert(provider, index);
        }
        store
    }

    #[test]
    fn a_gitlab_token_without_its_instance_is_not_connected() {
        let bound = store_with(Provider::GitLab, Some("gitlab.example.com"));
        assert!(bound.summary(Provider::GitLab).unwrap().has_default);
        let unbound = store_with(Provider::GitLab, None);
        let summary = unbound.summary(Provider::GitLab).unwrap();
        assert!(!summary.has_default);
        assert_eq!(summary.default_login, None);
        assert!(
            store_with(Provider::GitHub, None)
                .summary(Provider::GitHub)
                .unwrap()
                .has_default
        );
    }

    fn linked_store() -> TokenStore {
        let store = TokenStore::default();
        {
            let mut inner = store.lock().unwrap();
            let mut index = Index {
                default_login: Some("octocat".into()),
                default_gh: true,
                ..Index::default()
            };
            index.owners.insert("acme".into(), "octocat".into());
            index.owners.insert("corp".into(), "work-me".into());
            index.gh_owners.insert("acme".into());
            inner.indexes.insert(Provider::GitHub, index);
            inner
                .cache
                .insert((Provider::GitHub, Slot::Default), "gho_cached".into());
            inner
                .gh_checked
                .insert((Provider::GitHub, Slot::Default), Instant::now());
            inner.cache.insert(
                (Provider::GitHub, Slot::Owner(Owner::parse("corp").unwrap())),
                "ghp_stored".into(),
            );
        }
        store
    }

    #[test]
    fn index_decode_keeps_gh_links_only_for_known_slots() {
        let raw = r#"{
            "default_login": "octocat",
            "default_gh": true,
            "owners": { "acme": "octocat" },
            "gh_owners": ["acme", "ghost", "Bad:Owner"]
        }"#;
        let index = Index::decode(raw);
        assert!(index.default_gh);
        assert_eq!(index.gh_owners.iter().collect::<Vec<_>>(), ["acme"]);
        assert_eq!(index.gh_login(&Slot::Default), Some("octocat"));
        // A link without the login it is pinned to is dropped.
        assert!(!Index::decode(r#"{ "default_gh": true }"#).default_gh);
        // Indexes written before GitHub CLI links still load, unlinked.
        let old = Index::decode(r#"{ "default_login": "octocat", "owners": { "acme": "x" } }"#);
        assert_eq!(old.gh_login(&Slot::Default), None);
        assert_eq!(
            old.gh_login(&Slot::Owner(Owner::parse("acme").unwrap())),
            None
        );
        // Unlinked indexes encode exactly as before.
        assert!(!old.encode().contains("gh"));
    }

    #[test]
    fn linked_slots_are_reported_without_reading_the_keychain() {
        let store = linked_store();
        store.forget_cached(Provider::GitHub, &Slot::Default);
        let summary = store.summary(Provider::GitHub).unwrap();
        assert!(summary.has_default);
        assert_eq!(summary.default_login.as_deref(), Some("octocat"));
        assert_eq!(summary.default_source, Some(Source::GitHubCli));
        let sources: Vec<_> = summary
            .owners
            .iter()
            .map(|o| (o.owner.as_str(), o.source))
            .collect();
        assert_eq!(
            sources,
            [("acme", Source::GitHubCli), ("corp", Source::Token)]
        );
    }

    #[test]
    fn linked_tokens_are_served_from_memory() {
        let store = linked_store();
        assert!(matches!(
            store.resolve(Provider::GitHub, None).unwrap(),
            Resolved::Token { token, slot: Slot::Default } if token == "gho_cached"
        ));
        // A stored per-owner token is unaffected by links elsewhere.
        let corp = Owner::parse("corp").unwrap();
        assert!(matches!(
            store.resolve(Provider::GitHub, Some(&corp)).unwrap(),
            Resolved::Token { token, slot } if token == "ghp_stored" && slot == Slot::Owner(corp.clone())
        ));
        // Without a checked token in memory, the caller is told to ask gh,
        // for the pinned login.
        store.forget_cached(Provider::GitHub, &Slot::Default);
        assert!(matches!(
            store.resolve(Provider::GitHub, None).unwrap(),
            Resolved::FromGh { login, slot: Slot::Default } if login == "octocat"
        ));
        let acme = Owner::parse("acme").unwrap();
        assert!(matches!(
            store.resolve(Provider::GitHub, Some(&acme)).unwrap(),
            Resolved::FromGh { login, .. } if login == "octocat"
        ));
    }

    #[test]
    fn gh_tokens_are_only_remembered_for_the_linked_account() {
        let store = linked_store();
        store.forget_cached(Provider::GitHub, &Slot::Default);
        store.remember_gh(
            Provider::GitHub,
            &Slot::Default,
            "someone-else",
            "gho_other",
        );
        assert!(!store
            .lock()
            .unwrap()
            .cache
            .contains_key(&(Provider::GitHub, Slot::Default)));
        store.remember_gh(Provider::GitHub, &Slot::Default, "octocat", "gho_fresh");
        assert_eq!(
            store.cached_gh(Provider::GitHub, &Slot::Default, "octocat"),
            Some(("gho_fresh".to_owned(), true))
        );
        assert_eq!(
            store.cached_gh(Provider::GitHub, &Slot::Default, "someone-else"),
            None
        );
        // Not linked: nothing is cached, so a stored token cannot be shadowed.
        let corp = Slot::Owner(Owner::parse("corp").unwrap());
        store.forget_cached(Provider::GitHub, &corp);
        store.remember_gh(Provider::GitHub, &corp, "work-me", "gho_other");
        assert!(!store
            .lock()
            .unwrap()
            .cache
            .contains_key(&(Provider::GitHub, corp)));
    }

    #[test]
    fn gh_tokens_are_rechecked_with_gh_after_a_while() {
        let store = linked_store();
        store.remember_gh(Provider::GitHub, &Slot::Default, "octocat", "gho_fresh");
        assert!(matches!(
            store.resolve(Provider::GitHub, None).unwrap(),
            Resolved::Token { token, .. } if token == "gho_fresh"
        ));
        // Once the check is old, gh is asked again (so `gh auth logout`
        // takes effect), though the old token is still known for comparison.
        let aged = Instant::now() - CLI_RECHECK - Duration::from_secs(1);
        store
            .lock()
            .unwrap()
            .gh_checked
            .insert((Provider::GitHub, Slot::Default), aged);
        assert!(matches!(
            store.resolve(Provider::GitHub, None).unwrap(),
            Resolved::FromGh { login, .. } if login == "octocat"
        ));
        assert_eq!(
            store.cached_gh(Provider::GitHub, &Slot::Default, "octocat"),
            Some(("gho_fresh".to_owned(), false))
        );
        // Never confirmed by gh at all (as after a restart): ask gh.
        store
            .lock()
            .unwrap()
            .gh_checked
            .remove(&(Provider::GitHub, Slot::Default));
        assert!(matches!(
            store.resolve(Provider::GitHub, None).unwrap(),
            Resolved::FromGh { .. }
        ));
    }

    #[test]
    fn index_decode_caps_owner_count() {
        let owners: BTreeMap<String, String> = (0..MAX_OWNER_TOKENS + 5)
            .map(|i| (format!("org{i}"), "me".to_owned()))
            .collect();
        let raw = serde_json::json!({ "owners": owners }).to_string();
        assert_eq!(Index::decode(&raw).owners.len(), MAX_OWNER_TOKENS);
    }

    use crate::glab::GlabToken;
    use std::time::SystemTime;

    /// Keychain calls in these tests go to [`test_keychain`], never to the
    /// real keychain.
    fn mock_keychain() {
        test_keychain::install();
    }

    const GL: Provider = Provider::GitLab;

    fn h(s: &str) -> Host {
        Host::parse(s).unwrap()
    }

    fn glab_token(token: &str, expires: Option<SystemTime>) -> GlabToken {
        GlabToken {
            token: token.into(),
            expires,
            read_at: Instant::now(),
        }
    }

    /// The default GitLab slot linked to glab's account `login` on `host`,
    /// with nothing in memory yet (as after a restart).
    fn glab_store(host: &str, login: &str) -> TokenStore {
        mock_keychain();
        let store = TokenStore::default();
        store.lock().unwrap().indexes.insert(
            GL,
            Index {
                default_login: Some(login.into()),
                host: Some(host.into()),
                default_glab: true,
                ..Index::default()
            },
        );
        store
    }

    /// Makes glab's last confirmation of the cached token `by` older.
    fn age_glab_check(store: &TokenStore, by: Duration) {
        let mut inner = store.lock().unwrap();
        let cached = inner.glab.as_mut().expect("a glab token is cached");
        cached.checked_at -= by;
    }

    /// What `resolve_with_host` picks: `Ok((token, host))` for a token to
    /// send, `Err((host, login))` when glab must be asked.
    fn picked(store: &TokenStore) -> std::result::Result<(String, String), (String, String)> {
        match store.resolve_with_host(GL).unwrap() {
            ResolvedGitLab::Token { token, host, .. } => Ok((token, host.as_str().to_owned())),
            ResolvedGitLab::FromGlab { host, login } => Err((host.as_str().to_owned(), login)),
        }
    }

    fn ask_glab(
        host: &str,
        login: &str,
    ) -> std::result::Result<(String, String), (String, String)> {
        Err((host.to_owned(), login.to_owned()))
    }

    fn send(token: &str, host: &str) -> std::result::Result<(String, String), (String, String)> {
        Ok((token.to_owned(), host.to_owned()))
    }

    #[test]
    fn index_keeps_a_glab_link_with_its_instance() {
        let raw = r#"{"default_login":"jane.doe","host":"gitlab.example.com:8443","owners":{},"default_glab":true}"#;
        let index = Index::decode(raw);
        assert!(index.default_glab);
        assert_eq!(index.host.as_deref(), Some("gitlab.example.com:8443"));
        assert_eq!(index.default_login.as_deref(), Some("jane.doe"));
        assert_eq!(Index::decode(&index.encode()), index);
        // A link is pinned to an account on an instance; without either it
        // is dropped rather than guessed.
        for raw in [
            r#"{"host":"gitlab.example.com","default_glab":true}"#,
            r#"{"default_login":"bad login","host":"gitlab.example.com","default_glab":true}"#,
            r#"{"default_login":"jane.doe","default_glab":true}"#,
            r#"{"default_login":"jane.doe","host":"http://gitlab.example.com","default_glab":true}"#,
        ] {
            assert!(!Index::decode(raw).default_glab, "{raw}");
        }
    }

    #[test]
    fn indexes_without_glab_links_load_and_encode_as_before() {
        // Written by IsItGreen 0.3.1.
        let gitlab = r#"{"default_login":"jane.doe","host":"gitlab.example.com","owners":{}}"#;
        let index = Index::decode(gitlab);
        assert!(!index.default_glab);
        assert_eq!(index.host.as_deref(), Some("gitlab.example.com"));
        assert_eq!(index.encode(), gitlab);
        let github = r#"{"default_login":"octocat","owners":{"acme":"octocat"},"default_gh":true,"gh_owners":["acme"]}"#;
        assert_eq!(Index::decode(github).encode(), github);
    }

    #[test]
    fn a_glab_linked_slot_is_reported_and_served_by_glab() {
        let store = glab_store("gitlab.example.com", "jane.doe");
        let summary = store.summary(GL).unwrap();
        assert!(summary.has_default);
        assert_eq!(summary.default_source, Some(Source::GitLabCli));
        assert_eq!(summary.default_login.as_deref(), Some("jane.doe"));
        assert_eq!(summary.host.as_deref(), Some("gitlab.example.com"));
        assert_eq!(
            serde_json::to_value(summary.default_source).unwrap(),
            "glab"
        );
        // Nothing in memory: the caller must ask glab, for the pinned
        // account on the linked instance.
        assert_eq!(picked(&store), ask_glab("gitlab.example.com", "jane.doe"));
        store.remember_glab(
            &h("gitlab.example.com"),
            "jane.doe",
            &glab_token("glpat-fresh0123456789ab", None),
        );
        assert_eq!(
            picked(&store),
            send("glpat-fresh0123456789ab", "gitlab.example.com")
        );
    }

    #[test]
    fn a_token_read_before_a_relink_is_never_used_after_it() {
        // The read for (A, X) finishes after the link moved to another
        // instance, another account, or both.
        for (host, login) in [
            ("gitlab.other.com", "jane.doe"),
            ("gitlab.example.com", "john.roe"),
            ("gitlab.other.com", "john.roe"),
        ] {
            let store = glab_store("gitlab.example.com", "jane.doe");
            store
                .link_glab(
                    &h(host),
                    login,
                    &glab_token("glpat-relinked0123456789", None),
                )
                .unwrap();
            store.forget_cached(GL, &Slot::Default);
            store.remember_glab(
                &h("gitlab.example.com"),
                "jane.doe",
                &glab_token("glpat-stale0123456789ab", None),
            );
            assert_eq!(picked(&store), ask_glab(host, login), "{host} {login}");
            assert!(
                store.cached_glab(&h(host), login).is_none(),
                "{host} {login}"
            );
            assert!(store
                .cached_glab(&h("gitlab.example.com"), "jane.doe")
                .is_none());
        }
    }

    #[test]
    fn glab_tokens_are_rechecked_after_a_while_or_near_their_expiry() {
        let store = glab_store("gitlab.example.com", "jane.doe");
        let host = h("gitlab.example.com");
        let now = SystemTime::now();
        let cases = [
            ("no expiry", None, true),
            ("an hour left", Some(now + Duration::from_secs(3600)), true),
            ("a minute left", Some(now + Duration::from_secs(60)), true),
            (
                "ten seconds left",
                Some(now + Duration::from_secs(10)),
                false,
            ),
            ("expired", Some(now - Duration::from_secs(10)), false),
        ];
        for (name, expires, fresh) in cases {
            store.remember_glab(
                &host,
                "jane.doe",
                &glab_token("glpat-expiring0123456789", expires),
            );
            let expected = if fresh {
                send("glpat-expiring0123456789", "gitlab.example.com")
            } else {
                ask_glab("gitlab.example.com", "jane.doe")
            };
            assert_eq!(picked(&store), expected, "{name}");
        }
        // A token with no expiry still goes back to glab once the check is
        // a minute old, so `glab auth logout` disconnects IsItGreen.
        store.remember_glab(
            &host,
            "jane.doe",
            &glab_token("glpat-checked0123456789", None),
        );
        age_glab_check(&store, CLI_RECHECK + Duration::from_secs(1));
        assert_eq!(picked(&store), ask_glab("gitlab.example.com", "jane.doe"));
        // The minute counts from when glab gave the token, not from when
        // checking it with GitLab finished.
        let mut slow = glab_token("glpat-slowcheck0123456789", None);
        slow.read_at -= CLI_RECHECK + Duration::from_secs(1);
        store.remember_glab(&host, "jane.doe", &slow);
        assert_eq!(picked(&store), ask_glab("gitlab.example.com", "jane.doe"));
    }

    #[test]
    fn storing_a_token_ends_the_glab_link() {
        let store = glab_store("gitlab.example.com", "jane.doe");
        store.remember_glab(
            &h("gitlab.example.com"),
            "jane.doe",
            &glab_token("glpat-fromglab0123456789", None),
        );
        store
            .set(
                GL,
                &Slot::Default,
                "glpat-pasted0123456789ab",
                "jane.doe",
                Some(&h("gitlab.other.com")),
            )
            .unwrap();
        assert_eq!(
            store.summary(GL).unwrap().default_source,
            Some(Source::Token)
        );
        assert_eq!(
            picked(&store),
            send("glpat-pasted0123456789ab", "gitlab.other.com")
        );
        assert!(!store
            .lock()
            .unwrap()
            .index(GL)
            .unwrap()
            .encode()
            .contains("glab"));
    }

    #[test]
    fn removing_the_connection_ends_the_glab_link() {
        let store = glab_store("gitlab.example.com", "jane.doe");
        store.remember_glab(
            &h("gitlab.example.com"),
            "jane.doe",
            &glab_token("glpat-fromglab0123456789", None),
        );
        store.clear(GL, &Slot::Default).unwrap();
        assert!(!store.summary(GL).unwrap().has_default);
        assert!(matches!(store.resolve_with_host(GL), Err(Error::NoToken)));
        assert!(store
            .cached_glab(&h("gitlab.example.com"), "jane.doe")
            .is_none());
    }

    #[test]
    fn linking_glab_replaces_a_stored_token_and_its_instance() {
        mock_keychain();
        let store = TokenStore::default();
        store
            .set(
                GL,
                &Slot::Default,
                "glpat-pasted0123456789ab",
                "jane.doe",
                Some(&h("gitlab.example.com")),
            )
            .unwrap();
        store
            .link_glab(
                &h("gitlab.other.com:8443"),
                "john.roe",
                &glab_token("glpat-linked0123456789ab", None),
            )
            .unwrap();
        let summary = store.summary(GL).unwrap();
        assert_eq!(summary.default_source, Some(Source::GitLabCli));
        assert_eq!(summary.host.as_deref(), Some("gitlab.other.com:8443"));
        assert_eq!(summary.default_login.as_deref(), Some("john.roe"));
        // The pasted token is never sent again, to either instance.
        store.forget_cached(GL, &Slot::Default);
        assert_eq!(
            picked(&store),
            ask_glab("gitlab.other.com:8443", "john.roe")
        );
        let saved = Index::decode(&store.lock().unwrap().index(GL).unwrap().encode());
        assert!(saved.default_glab);
    }

    /// The index as stored in the keychain, which is what a restart reads.
    fn stored_index(provider: Provider) -> Index {
        Index::decode(&test_keychain::stored(&index_account(provider)).unwrap())
    }

    /// Links over a stored token while deleting it fails, and the save that
    /// would undo the link fails too when `rollback_fails`.
    fn link_while_delete_fails(
        rollback_fails: bool,
        link: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        test_keychain::fail_delete(true);
        // The link's own save works; only the one undoing it may fail.
        test_keychain::fail_set_after(rollback_fails.then_some(1));
        let result = link();
        test_keychain::fail_delete(false);
        test_keychain::fail_set_after(None);
        result
    }

    const GH_STORED: &str = "ghp_stored0123456789abcdef";
    const GL_STORED: &str = "glpat-pasted0123456789ab";

    fn gh_with_stored_token() -> TokenStore {
        mock_keychain();
        let store = TokenStore::default();
        store
            .set(GH, &Slot::Default, GH_STORED, "octocat", None)
            .unwrap();
        store
    }

    fn gl_with_stored_token() -> TokenStore {
        mock_keychain();
        let store = TokenStore::default();
        store
            .set(
                GL,
                &Slot::Default,
                GL_STORED,
                "jane.doe",
                Some(&h("gitlab.example.com")),
            )
            .unwrap();
        store
    }

    const GH: Provider = Provider::GitHub;

    #[test]
    fn a_gh_link_whose_old_token_cannot_be_deleted_is_undone() {
        let store = gh_with_stored_token();
        let before = stored_index(GH);
        let result = link_while_delete_fails(false, || {
            store.link_gh(GH, &Slot::Default, "octocat", "gho_linked")
        });
        assert!(matches!(result, Err(Error::Keyring(_))));
        assert_eq!(stored_index(GH), before);
        assert_eq!(store.lock().unwrap().index(GH).unwrap().clone(), before);
        assert!(matches!(
            store.resolve(GH, None).unwrap(),
            Resolved::Token { token, .. } if token == GH_STORED
        ));
    }

    #[test]
    fn a_gh_link_that_cannot_be_undone_is_kept() {
        let store = gh_with_stored_token();
        let result = link_while_delete_fails(true, || {
            store.link_gh(GH, &Slot::Default, "octocat", "gho_linked")
        });
        assert!(result.is_ok());
        assert!(matches!(
            store.resolve(GH, None).unwrap(),
            Resolved::Token { token, .. } if token == "gho_linked"
        ));
        // After a restart the same link is in force.
        let restarted = TokenStore::default();
        assert!(matches!(
            restarted.resolve(GH, None).unwrap(),
            Resolved::FromGh { login, slot: Slot::Default } if login == "octocat"
        ));
    }

    #[test]
    fn a_glab_link_whose_old_token_cannot_be_deleted_is_undone() {
        let store = gl_with_stored_token();
        let before = stored_index(GL);
        let result = link_while_delete_fails(false, || {
            store.link_glab(
                &h("gitlab.other.com"),
                "jane.doe",
                &glab_token("glpat-linked", None),
            )
        });
        assert!(matches!(result, Err(Error::Keyring(_))));
        assert_eq!(stored_index(GL), before);
        assert_eq!(store.lock().unwrap().index(GL).unwrap().clone(), before);
        assert_eq!(
            picked(&store),
            Ok((GL_STORED.to_owned(), "gitlab.example.com".to_owned()))
        );
    }

    #[test]
    fn a_glab_link_that_cannot_be_undone_is_kept() {
        let store = gl_with_stored_token();
        let result = link_while_delete_fails(true, || {
            store.link_glab(
                &h("gitlab.other.com"),
                "jane.doe",
                &glab_token("glpat-linked", None),
            )
        });
        assert!(result.is_ok());
        assert_eq!(
            picked(&store),
            Ok(("glpat-linked".to_owned(), "gitlab.other.com".to_owned()))
        );
        // After a restart the same link is in force.
        assert_eq!(
            picked(&TokenStore::default()),
            Err(("gitlab.other.com".to_owned(), "jane.doe".to_owned()))
        );
    }
}
