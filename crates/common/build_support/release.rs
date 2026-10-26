/*
 * SPDX-FileCopyrightText: 2026 Stalwart contributors
 *
 * SPDX-License-Identifier: AGPL-3.0-only
 */

use std::{env, fs, path::Path, process::Command};

const SOURCE_OVERRIDE: &str = "STALWART_BUILD_SOURCE_COMMIT";

/// Read the deliberately flat, quoted-string release metadata file.
fn field<'a>(content: &'a str, key: &str) -> &'a str {
    let mut values = content.lines().filter_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name.trim() == key).then_some(value.trim())
    });
    let value = values
        .next()
        .unwrap_or_else(|| panic!("Release metadata is missing {key}"));
    assert!(values.next().is_none(), "Duplicate release metadata: {key}");
    let value = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or_else(|| panic!("Release metadata {key} must be a quoted string"));
    assert!(
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte)),
        "Invalid release metadata: {key}"
    );
    value
}

fn full_commit(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())
        .flatten()
}

fn watch(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
}

pub fn emit(root: &Path) {
    let metadata_path = root.join("fork-release.toml");
    let manifest_path = root.join("crates/main/Cargo.toml");
    watch(&metadata_path);
    watch(&manifest_path);
    println!("cargo:rerun-if-env-changed={SOURCE_OVERRIDE}");
    let metadata = fs::read_to_string(metadata_path).expect("Failed to read fork-release.toml");
    let version = field(&metadata, "version");
    let upstream_tag = field(&metadata, "upstream_tag");
    let upstream_commit = field(&metadata, "upstream_commit");
    assert!(full_commit(upstream_commit), "Invalid upstream_commit");
    let manifest = fs::read_to_string(manifest_path).expect("Failed to read shipping manifest");
    let mut lines = manifest.lines();
    assert!(
        lines.by_ref().any(|line| line.trim() == "[package]"),
        "Missing shipping package metadata"
    );
    // Table headers start a line; brackets inside package array values do not end the table.
    let package = lines
        .take_while(|line| !line.trim_start().starts_with('['))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        field(&package, "version"),
        version,
        "Shipping manifest and fork-release.toml versions disagree"
    );

    let override_commit = env::var_os(SOURCE_OVERRIDE).map(|value| {
        let value = value
            .into_string()
            .unwrap_or_else(|_| panic!("{SOURCE_OVERRIDE} must be a full hexadecimal commit"));
        assert!(
            full_commit(&value),
            "{SOURCE_OVERRIDE} must be a full hexadecimal commit"
        );
        value.to_ascii_lowercase()
    });

    // A source archive inside another checkout must not borrow that checkout's identity.
    let is_checkout = git(root, &["rev-parse", "--show-toplevel"])
        .and_then(|path| Path::new(path.trim()).canonicalize().ok())
        == root.canonicalize().ok();
    let (source_commit, source_state) = if is_checkout {
        let commit = git(root, &["rev-parse", "HEAD"])
            .expect("Failed to resolve checkout HEAD")
            .trim()
            .to_string();
        assert!(
            full_commit(&commit),
            "Git returned an invalid source commit"
        );
        if let Some(override_commit) = override_commit {
            assert_eq!(
                override_commit, commit,
                "{SOURCE_OVERRIDE} disagrees with checkout HEAD"
            );
        }
        for name in ["HEAD", "index", "packed-refs", "refs"] {
            let path = git(
                root,
                &["rev-parse", "--path-format=absolute", "--git-path", name],
            )
            .expect("Failed to locate Git build metadata");
            watch(Path::new(path.trim()));
        }
        // Track dirty-state changes too, not only branch/ref changes. NUL delimiters preserve spaces.
        let files = git(root, &["ls-files", "-z"]).expect("Failed to enumerate source files");
        for file in files.split('\0').filter(|file| !file.is_empty()) {
            watch(&root.join(file));
        }
        let state = git(root, &["status", "--porcelain", "--untracked-files=no"])
            .expect("Failed to inspect checkout state");
        (commit, if state.is_empty() { "clean" } else { "dirty" })
    } else if let Some(commit) = override_commit {
        (commit, "provided")
    } else {
        ("unknown".to_string(), "unknown")
    };
    let identity = format!(
        "Stalwart Server fork v{version} (upstream {upstream_tag} {upstream_commit}; \
         source {source_commit}; {source_state})"
    );
    for (name, value) in [
        ("STALWART_FORK_VERSION", version),
        ("STALWART_UPSTREAM_TAG", upstream_tag),
        ("STALWART_UPSTREAM_COMMIT", upstream_commit),
        ("STALWART_SOURCE_COMMIT", source_commit.as_str()),
        ("STALWART_SOURCE_STATE", source_state),
        ("STALWART_BUILD_IDENTITY", identity.as_str()),
    ] {
        println!("cargo:rustc-env={name}={value}");
    }
}
