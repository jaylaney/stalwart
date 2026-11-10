/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use super::{
    BODY_WORD, COLD_PLUMBING_COUNT, generated,
    system::{DESIGN, SEED},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize)]
struct Message {
    file: String,
    sha256: String,
    bytes: usize,
    attachment_bytes: usize,
}

#[derive(Serialize)]
struct Manifest {
    version: u32,
    seed: u64,
    count: usize,
    body_word: &'static str,
    messages: Vec<Message>,
    corpus_sha256: String,
}

pub(super) fn run() -> Result<(), Box<dyn Error>> {
    let directory = PathBuf::from(
        std::env::var_os("SEARCH_BENCH_EXPORT_DIR").ok_or("SEARCH_BENCH_EXPORT_DIR is required")?,
    );
    let count = match std::env::var("SEARCH_BENCH_EXPORT_COUNT") {
        Err(std::env::VarError::NotPresent) => DESIGN,
        Ok(value) if value == COLD_PLUMBING_COUNT.to_string() => COLD_PLUMBING_COUNT,
        _ => return Err("SEARCH_BENCH_EXPORT_COUNT, if set, must be 5 for plumbing".into()),
    };
    export(&directory, count)
}

fn export(directory: &Path, count: usize) -> Result<(), Box<dyn Error>> {
    match fs::create_dir(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !directory.symlink_metadata()?.is_dir() || directory.read_dir()?.next().is_some() {
                return Err("corpus output must be a fresh or empty directory".into());
            }
        }
        Err(error) => return Err(error.into()),
    }

    let mut digest = Sha256::new();
    let mut messages = Vec::with_capacity(count);
    for (ordinal, message) in generated(SEED).take(count).enumerate() {
        let file = format!("{ordinal:06}.eml");
        File::create_new(directory.join(&file))?.write_all(&message.raw)?;
        digest.update(&message.raw);
        messages.push(Message {
            file,
            sha256: hex(&Sha256::digest(&message.raw)),
            bytes: message.raw.len(),
            attachment_bytes: message.attachment_bytes,
        });
    }
    let corpus_sha256 = hex(&digest.finalize());
    let manifest = Manifest {
        version: 1,
        seed: SEED,
        count,
        body_word: BODY_WORD,
        messages,
        corpus_sha256,
    };
    let mut file = File::create_new(directory.join("manifest.json"))?;
    serde_json::to_writer(&mut file, &manifest)?;
    file.write_all(b"\n")?;
    println!(
        "Exported seed {SEED}, {count} messages, corpus SHA-256 {}",
        manifest.corpus_sha256
    );
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
