//! The vendor boundary, checked against the crate's own source.
//!
//! These read the source rather than run it. What they catch is a call site
//! that pastes a host or a key where it is convenient, and no behavioral
//! test can see that.

use std::path::{Path, PathBuf};

/// Every literal that means "this exact vendor": a host, a credential, or an
/// id the vendor assigned. Not here: the per-clip ids a `MotionSource`
/// carries, which are content the animation library declares rather than an
/// address.
///
/// Spelled out rather than imported from the providers, so moving a constant
/// out of one cannot take this check with it. The third test below is what
/// keeps every entry honest.
const VENDOR_LITERALS: [&str; 12] = [
    "mixamo.com",
    "meshy.ai",
    "openai.com",
    "MARROWFALL_MIXAMO",
    "MARROWFALL_MESHY",
    "MARROWFALL_OPENAI",
    "MESHY_API_KEY",
    "OPENAI_API_KEY",
    "X-Api-Key",
    "mixamo2",
    "4f5d21e1-4ccc-41f1-b35b-fb2547bd8493",
    "gpt-image",
];

#[test]
fn no_vendor_host_id_or_key_lives_outside_the_providers() {
    let outside: Vec<PathBuf> = rust_files(&src())
        .into_iter()
        .filter(|path| !path.starts_with(providers()))
        .collect();
    // A walk that finds nothing would pass without proving anything, and
    // `cli.rs` is the file that used to hold a Mixamo URL of its own.
    assert!(
        outside.iter().any(|path| path.ends_with("cli.rs")),
        "found no cli.rs to check: {outside:#?}"
    );

    for path in outside {
        let source = std::fs::read_to_string(&path).unwrap();
        for literal in VENDOR_LITERALS {
            assert!(
                !source.contains(literal),
                "{} names {literal:?}. A vendor host, id or key belongs in \
                 src/providers/, and every caller imports it from there.",
                path.display()
            );
        }
    }
}

/// The other half of the boundary, and the reason `http.rs` exists: two
/// vendors can share a helper without one of them depending on the other.
#[test]
fn no_provider_reaches_into_another_provider() {
    let names = provider_names();
    for path in rust_files(&providers()) {
        let owner = provider_of(&path);
        // `providers/mod.rs` states the contract, so it names every vendor.
        if !names.contains(&owner) {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        // A provider reaches its own parts through `super::` and the shared
        // helpers through `crate::http`, so the word that leads every path to
        // a sibling never appears at all. This catches the grouped import,
        // `use crate::providers::{meshy, openai};`, which a needle per
        // sibling name would miss.
        assert!(
            !source.contains("providers"),
            "{} names `providers`, so it can reach a sibling. Use `super::` \
             for its own parts and `crate::http` for anything shared.",
            path.display()
        );
        for other in names.iter().filter(|other| **other != owner) {
            assert!(
                !source.contains(&format!("{other}::")),
                "{} names the {other} provider. Anything two vendors share \
                 belongs in src/http.rs.",
                path.display()
            );
        }
    }
}

/// The control for [`VENDOR_LITERALS`]: a typo in one entry would make it
/// dead, and the first test would stay green forever.
#[test]
fn every_vendor_literal_is_one_a_provider_really_uses() {
    let sources: Vec<String> = rust_files(&providers())
        .iter()
        .map(|path| std::fs::read_to_string(path).unwrap())
        .collect();

    for literal in VENDOR_LITERALS {
        assert!(
            sources.iter().any(|source| source.contains(literal)),
            "no provider names {literal:?}, so it is spelled wrong or no \
             longer used. Correct it or delete the entry."
        );
    }
}

fn src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn providers() -> PathBuf {
    src().join("providers")
}

/// The vendors, read from the directory rather than listed, so adding one
/// cannot quietly narrow the check above.
fn provider_names() -> Vec<String> {
    let names: Vec<String> = std::fs::read_dir(providers())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.file_name().is_some_and(|name| name != "mod.rs"))
        .filter_map(|path| Some(path.file_stem()?.to_string_lossy().into_owned()))
        .collect();
    assert!(
        names.len() >= 3,
        "read {} provider(s) from src/providers/, so the walk is broken",
        names.len()
    );
    names
}

/// Which provider owns a file: the first path component under `providers/`,
/// without its extension.
fn provider_of(path: &Path) -> String {
    path.strip_prefix(providers())
        .unwrap()
        .components()
        .next()
        .unwrap()
        .as_os_str()
        .to_string_lossy()
        .trim_end_matches(".rs")
        .to_owned()
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }
    found
}
