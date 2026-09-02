//! Reading the Mixamo credential out of Chrome's own storage.
//!
//! The LevelDB is built here rather than read from a real profile, so these
//! run on CI with no Chrome installed, which is the point.

use std::path::Path;
use std::time::Duration;

use base64::Engine as _;
use rusty_leveldb::{DB, Options};
use xtask_art::providers::mixamo::session;

use crate::support::EnvGuard;

/// Seconds since the epoch, far enough out that these tests keep passing.
const FOREVER: i64 = 4_102_444_800; // 2100-01-01

fn base64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// A JWT carrying the standard `exp`, which Adobe does not send but other
/// providers do. Unsigned, because nothing here verifies a signature.
fn a_jwt(expires_at: i64) -> String {
    a_jwt_payload(&format!(r#"{{"sub":"someone","exp":{expires_at}}}"#))
}

/// A JWT of the shape Adobe IMS really issues: no `exp` at all, and
/// `created_at` plus `expires_in` in milliseconds, both as strings.
fn an_adobe_jwt(created_at_ms: i64, expires_in_ms: i64) -> String {
    a_jwt_payload(&format!(
        r#"{{"user_id":"someone","created_at":"{created_at_ms}","expires_in":"{expires_in_ms}"}}"#
    ))
}

fn a_jwt_payload(payload: &str) -> String {
    format!(
        "{}.{}.{}",
        base64url(br#"{"alg":"HS256"}"#),
        base64url(payload.as_bytes()),
        base64url(b"not a real signature")
    )
}

/// Chrome tags every stored value with its encoding: 1 is Latin-1.
fn latin1(text: &str) -> Vec<u8> {
    let mut bytes = vec![0x01];
    bytes.extend(text.bytes());
    bytes
}

fn utf16(text: &str) -> Vec<u8> {
    let mut bytes = vec![0x00];
    for unit in text.encode_utf16() {
        bytes.extend(unit.to_le_bytes());
    }
    bytes
}

/// One Chrome profile holding the given `origin\0\x01key` entries.
fn a_profile(chrome: &Path, profile: &str, entries: &[(&str, Vec<u8>)]) {
    let leveldb = chrome.join(profile).join("Local Storage/leveldb");
    std::fs::create_dir_all(&leveldb).unwrap();
    let mut db = DB::open(&leveldb, Options::default()).unwrap();
    for (key, value) in entries {
        db.put(key.as_bytes(), value).unwrap();
    }
    db.flush().unwrap();
}

fn mixamo_key(name: &str) -> String {
    format!("_https://www.mixamo.com\u{0}\u{1}{name}")
}

// --- Finding a token among stored values ---------------------------------

#[test]
fn a_stored_jwt_is_found_by_its_shape() {
    let jwt = a_jwt(FOREVER);
    let token = session::newest_token(std::slice::from_ref(&jwt)).unwrap();
    assert_eq!(token.expose_secret(), jwt);
}

#[test]
fn a_jwt_is_found_inside_the_json_adobe_wraps_it_in() {
    // The storage key is not documented, so the token is found by shape.
    let jwt = a_jwt(FOREVER);
    let stored = format!(r#"{{"tokenValue":"{jwt}","user":"someone@example.com"}}"#);
    assert_eq!(
        session::newest_token(&[stored]).unwrap().expose_secret(),
        jwt
    );
}

#[test]
fn the_longest_lived_token_wins() {
    let older = a_jwt(FOREVER - 3600);
    let newer = a_jwt(FOREVER);
    let found = session::newest_token(&[older, newer.clone()]).unwrap();
    assert_eq!(found.expose_secret(), newer, "a stale session lingers");
}

#[test]
fn an_expired_token_reads_as_no_token() {
    // Adobe's last a day, so a stale one must re-prompt rather than 401.
    assert!(session::newest_token(&[a_jwt(1_600_000_000)]).is_none());
}

#[test]
fn anything_that_is_not_a_jwt_is_ignored() {
    let payload = base64url(br#"{"exp":4102444800}"#);
    let values = [
        "not a token at all".to_owned(),
        format!("{payload}.{payload}"), // two segments
        format!("{payload}.{payload}.{payload}.{payload}"), // four
        "aaa.!!!not base64!!!.bbb".to_owned(),
        a_jwt_payload(r#"{"sub":"no expiry claim"}"#),
        a_jwt_payload("this payload is not json"),
    ];
    assert!(session::newest_token(&values).is_none());
}

#[test]
fn a_payload_whose_length_needs_padding_still_decodes() {
    // base64url in a JWT carries no `=`, so the decoder must not want any.
    let jwt = a_jwt_payload(r#"{"exp":4102444800,"pad":"ab"}"#);
    assert_eq!(
        jwt.split('.').nth(1).unwrap().len() % 4,
        3,
        "unpadded length"
    );
    assert!(session::newest_token(&[jwt]).is_some());
}

#[test]
fn nothing_stored_means_nothing_found() {
    assert!(session::newest_token(&[]).is_none());
}

#[test]
fn a_token_never_prints_itself() {
    // A leaked bearer token is an Adobe account credential.
    let jwt = a_jwt(FOREVER);
    let token = session::newest_token(std::slice::from_ref(&jwt)).unwrap();
    assert!(!format!("{token:?}").contains(&jwt), "got: {token:?}");
}

// --- Reading Chrome's profiles -------------------------------------------

#[test]
fn a_live_session_yields_the_token_it_stored() {
    let dir = tempfile::tempdir().unwrap();
    let jwt = a_jwt(FOREVER);
    a_profile(
        dir.path(),
        "Default",
        &[(&mixamo_key("adobeid"), latin1(&jwt))],
    );

    let token = session::token_in(dir.path()).unwrap().unwrap();
    assert_eq!(token.expose_secret(), jwt);
}

#[test]
fn a_utf16_value_is_decoded_too() {
    let dir = tempfile::tempdir().unwrap();
    let jwt = a_jwt(FOREVER);
    let stored = format!(r#"{{"café":"{jwt}"}}"#);
    a_profile(
        dir.path(),
        "Default",
        &[(&mixamo_key("session"), utf16(&stored))],
    );

    assert_eq!(
        session::token_in(dir.path())
            .unwrap()
            .unwrap()
            .expose_secret(),
        jwt
    );
}

#[test]
fn a_value_tagged_with_an_encoding_nobody_documents_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let mut tagged = vec![0x07];
    tagged.extend(a_jwt(FOREVER).bytes());
    a_profile(dir.path(), "Default", &[(&mixamo_key("odd"), tagged)]);

    assert!(
        session::token_in(dir.path()).unwrap().is_none(),
        "Chrome's layout is a convention, so an unknown shape is not guessed at"
    );
}

#[test]
fn a_subdirectory_beside_the_storage_does_not_stop_the_read() {
    let dir = tempfile::tempdir().unwrap();
    let jwt = a_jwt(FOREVER);
    a_profile(
        dir.path(),
        "Default",
        &[(&mixamo_key("adobeid"), latin1(&jwt))],
    );
    std::fs::create_dir(
        dir.path()
            .join("Default/Local Storage/leveldb")
            .join("something-else"),
    )
    .unwrap();

    assert_eq!(
        session::token_in(dir.path())
            .unwrap()
            .unwrap()
            .expose_secret(),
        jwt
    );
}

#[test]
fn another_sites_storage_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let entries = [(
        "_https://example.com\u{0}\u{1}token".to_owned(),
        latin1(&a_jwt(FOREVER)),
    )];
    a_profile(
        dir.path(),
        "Default",
        &entries
            .iter()
            .map(|(key, value)| (key.as_str(), value.clone()))
            .collect::<Vec<_>>(),
    );

    assert!(session::token_in(dir.path()).unwrap().is_none());
}

#[test]
fn every_profile_is_searched_and_the_freshest_wins() {
    let dir = tempfile::tempdir().unwrap();
    let older = a_jwt(FOREVER - 3600);
    let newer = a_jwt(FOREVER);
    a_profile(
        dir.path(),
        "Default",
        &[(&mixamo_key("id"), latin1(&older))],
    );
    a_profile(
        dir.path(),
        "Profile 2",
        &[(&mixamo_key("id"), latin1(&newer))],
    );

    let token = session::token_in(dir.path()).unwrap().unwrap();
    assert_eq!(token.expose_secret(), newer);
}

#[test]
fn a_logged_out_profile_yields_nothing() {
    let dir = tempfile::tempdir().unwrap();
    a_profile(dir.path(), "Default", &[]);
    assert!(session::token_in(dir.path()).unwrap().is_none());
}

#[test]
fn no_chrome_at_all_is_not_an_error() {
    // CI runs Linux with no browser installed.
    let dir = tempfile::tempdir().unwrap();
    assert!(
        session::token_in(&dir.path().join("nothing here"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn an_unreadable_storage_names_the_directory_and_not_the_contents() {
    let dir = tempfile::tempdir().unwrap();
    let leveldb = dir.path().join("Default/Local Storage/leveldb");
    std::fs::create_dir_all(&leveldb).unwrap();
    std::fs::write(leveldb.join("CURRENT"), "not a leveldb").unwrap();

    let error = format!("{:#}", session::token_in(dir.path()).unwrap_err());
    assert!(error.contains("leveldb"), "got: {error}");
}

// --- The credential the rest of the tool asks for -------------------------

#[test]
fn the_environment_overrides_chrome_entirely() {
    // The CI and test escape hatch: no browser, no LevelDB, no login.
    let mut env = EnvGuard::new();
    env.set("MARROWFALL_MIXAMO_TOKEN", "a-token-from-somewhere-else");
    let token = session::token().unwrap().unwrap();
    assert_eq!(token.expose_secret(), "a-token-from-somewhere-else");
}

#[test]
fn an_empty_override_is_treated_as_unset() {
    // `HOME` too, or this falls through to the developer's own Chrome and
    // passes or fails depending on whether they happen to be logged in.
    let home = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    env.set("HOME", home.path().to_str().unwrap());
    env.set("MARROWFALL_MIXAMO_TOKEN", "");
    assert!(session::token().unwrap().is_none());
}

#[test]
fn waiting_returns_at_once_when_the_session_is_already_there() {
    let dir = tempfile::tempdir().unwrap();
    let jwt = a_jwt(FOREVER);
    a_profile(
        dir.path(),
        "Default",
        &[(&mixamo_key("adobeid"), latin1(&jwt))],
    );

    let token = session::wait_for_token(dir.path(), Duration::from_secs(1)).unwrap();
    assert_eq!(token.expose_secret(), jwt);
}

#[test]
fn waiting_gives_up_and_says_what_the_user_has_to_do() {
    let dir = tempfile::tempdir().unwrap();
    let error = session::wait_for_token(dir.path(), Duration::ZERO)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Mixamo"), "got: {error}");
    assert!(error.contains("Chrome"), "got: {error}");
}

/// Adobe IMS sends no `exp`. Reading only that claim finds no token at all and
/// asks the user to log in again while they already are, which is the fault
/// this covers.
#[test]
fn an_adobe_token_is_found_though_it_carries_no_exp() {
    let day = 86_400_000;
    let created = (FOREVER - 3600) * 1000;
    let jwt = an_adobe_jwt(created, day);
    let token = session::newest_token(std::slice::from_ref(&jwt)).unwrap();
    assert_eq!(token.expose_secret(), jwt);
}

/// The expiry is `created_at + expires_in`, converted from milliseconds, so a
/// token issued a day and an hour ago has already lapsed.
#[test]
fn an_adobe_token_lapses_a_day_after_it_was_created() {
    let day = 86_400_000;
    let created = (unix_now() - 86_400 - 3600) * 1000;
    assert!(session::newest_token(&[an_adobe_jwt(created, day)]).is_none());
}

/// Adobe quotes both numbers. A provider that sends them bare must still work.
#[test]
fn millisecond_claims_are_read_whether_quoted_or_not() {
    let created = (FOREVER - 3600) * 1000;
    let bare = a_jwt_payload(&format!(
        r#"{{"created_at":{created},"expires_in":86400000}}"#
    ));
    assert!(session::newest_token(&[bare]).is_some());
}

/// A payload with neither shape is not a token, and must not be guessed at.
#[test]
fn a_payload_with_no_expiry_at_all_reads_as_no_token() {
    let jwt = a_jwt_payload(r#"{"user_id":"someone"}"#);
    assert!(session::newest_token(&[jwt]).is_none());
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}
