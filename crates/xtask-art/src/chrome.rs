//! Reads the Mixamo bearer token from Chrome's session storage, so nobody
//! pastes it from devtools every day. The token is read, handed over once, and
//! dropped: never printed, written, or put in an error. Chrome's layout is a
//! convention, so anything unexpected is skipped, not asserted on.
//! `MARROWFALL_MIXAMO_TOKEN` bypasses all of it.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use base64::Engine as _;
use rusty_leveldb::{DB, LdbIterator as _, Options};
use serde_json::Value;

/// Supplies the token directly, for CI and tests. Set, and Chrome is not read.
const TOKEN_ENV: &str = "MARROWFALL_MIXAMO_TOKEN";

/// Local Storage is keyed by origin, and this is the only one we look at.
const MIXAMO_ORIGIN: &str = "https://www.mixamo.com";

/// How often a browser login is checked for while waiting on the user.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// A bearer credential, kept out of every output the tool produces.
pub struct Token {
    secret: String,
    expires_at: i64,
}

impl Token {
    /// The credential itself. Every call site is a place it could leak, so
    /// this is deliberately awkward to type: pass it to a request, nothing
    /// else.
    pub fn expose_secret(&self) -> &str {
        &self.secret
    }
}

impl fmt::Debug for Token {
    /// Redacted, so no `{:?}` anywhere can leak it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Token(redacted, expires at {})", self.expires_at)
    }
}

/// The token to export with, from the environment or from Chrome.
pub fn mixamo_token() -> Result<Option<Token>> {
    if let Ok(secret) = std::env::var(TOKEN_ENV)
        && !secret.is_empty()
    {
        // Trusted as given: whoever set it knows what they are doing, and its
        // expiry is Mixamo's to judge.
        return Ok(Some(Token {
            secret,
            expires_at: 0,
        }));
    }
    token_in(&profiles_dir()?)
}

/// Where Chrome keeps one user's profiles. macOS only, as the pipeline is.
pub fn profiles_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set, so Chrome cannot be found")?;
    Ok(PathBuf::from(home).join("Library/Application Support/Google/Chrome"))
}

/// The freshest Mixamo token any profile under `chrome` holds.
///
/// A missing directory is not an error: no Chrome, or no login yet, both mean
/// the caller has to ask the user to log in.
pub fn token_in(chrome: &Path) -> Result<Option<Token>> {
    let mut values = Vec::new();
    for profile in profiles(chrome) {
        let leveldb = profile.join("Local Storage/leveldb");
        if leveldb.is_dir() {
            values.extend(stored_values(&leveldb)?);
        }
    }
    Ok(newest_token(&values))
}

/// Polls `chrome` until a session appears, or gives up and says why.
pub fn wait_for_token(chrome: &Path, within: Duration) -> Result<Token> {
    let deadline = Instant::now() + within;
    loop {
        if let Some(token) = token_in(chrome)? {
            return Ok(token);
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "no Mixamo session appeared in Chrome within {} seconds. \
             Log in at {MIXAMO_ORIGIN} and run this again.",
            within.as_secs()
        );
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// The freshest still-valid token among values Chrome has stored.
///
/// Found by shape rather than by key name: Adobe can rename its storage key
/// without breaking this, which matters on a surface nobody documents.
pub fn newest_token(values: &[String]) -> Option<Token> {
    let now = unix_now();
    values
        .iter()
        .flat_map(|value| jwt_candidates(value))
        .filter_map(|candidate| {
            Some(Token {
                secret: candidate.to_owned(),
                expires_at: expiry(candidate)?,
            })
        })
        .filter(|token| token.expires_at > now)
        .max_by_key(|token| token.expires_at)
}

/// `Default` first, then every `Profile *`, since a user can have several.
fn profiles(chrome: &Path) -> Vec<PathBuf> {
    let mut found = vec![chrome.join("Default")];
    let others = std::fs::read_dir(chrome).into_iter().flatten().flatten();
    let mut others: Vec<PathBuf> = others
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("Profile "))
        })
        .collect();
    others.sort();
    found.append(&mut others);
    found
}

/// Everything Chrome has stored for the Mixamo origin, decoded to text.
fn stored_values(leveldb: &Path) -> Result<Vec<String>> {
    // Chrome holds an exclusive lock on its own LevelDB while it runs, so the
    // copy is what gets opened. Deleted when this returns, either way.
    let scratch = tempfile::tempdir().context("making room to copy Chrome's storage")?;
    copy_files(leveldb, scratch.path())
        .with_context(|| format!("copying {}", leveldb.display()))?;

    let options = Options {
        create_if_missing: false,
        ..Options::default()
    };
    let mut db = DB::open(scratch.path(), options)
        .with_context(|| format!("opening a copy of {}", leveldb.display()))?;
    let mut entries = db
        .new_iter()
        .with_context(|| format!("reading {}", leveldb.display()))?;

    let prefix = format!("_{MIXAMO_ORIGIN}\u{0}\u{1}").into_bytes();
    let mut values = Vec::new();
    while let Some((key, value)) = entries.next() {
        if key.starts_with(&prefix) {
            values.extend(decode(&value));
        }
    }
    Ok(values)
}

/// Copies one flat directory of files. LevelDB has no subdirectories.
fn copy_files(from: &Path, to: &Path) -> Result<()> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// Strips the one byte encoding tag Chrome prefixes every value with.
///
/// `\x01` is Latin-1, `\x00` is UTF-16. Documented by forensic analysis
/// rather than by Google, so any other tag is skipped rather than guessed at.
fn decode(raw: &[u8]) -> Option<String> {
    match raw.split_first() {
        Some((0x01, text)) => Some(text.iter().map(|&byte| byte as char).collect()),
        Some((0x00, text)) => {
            let units: Vec<u16> = text
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            Some(String::from_utf16_lossy(&units))
        }
        _ => None,
    }
}

/// Substrings that could be a JWT: a token is usually wrapped in JSON.
fn jwt_candidates(value: &str) -> impl Iterator<Item = &str> {
    value.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
}

/// When a token stops working, in seconds since the epoch.
///
/// Adobe IMS carries no `exp` (RFC 7519 section 4.1.4). It sends `created_at`
/// and `expires_in` instead, both in milliseconds and both as strings, so
/// `exp` is the fallback rather than the rule.
///
/// The signature is deliberately not verified: Mixamo is both the audience
/// and the verifier, and all we need is to know when to prompt again.
fn expiry(jwt: &str) -> Option<i64> {
    let mut segments = jwt.split('.');
    let (_header, payload, _signature) = (segments.next()?, segments.next()?, segments.next()?);
    if segments.next().is_some() {
        return None;
    }
    // JWS uses base64url with the padding stripped (RFC 7515 section 2).
    let json = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims = serde_json::from_slice::<Value>(&json).ok()?;
    if let (Some(created), Some(lifetime)) =
        (millis(&claims, "created_at"), millis(&claims, "expires_in"))
    {
        return Some((created + lifetime) / 1000);
    }
    claims.get("exp")?.as_i64()
}

/// One millisecond claim, which Adobe writes as a string and others as a number.
fn millis(claims: &Value, name: &str) -> Option<i64> {
    let claim = claims.get(name)?;
    claim
        .as_i64()
        .or_else(|| claim.as_str()?.parse::<i64>().ok())
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}
