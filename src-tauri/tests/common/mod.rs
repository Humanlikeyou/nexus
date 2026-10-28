//! Shared helpers for the integration tests (included with `mod common;`).
#![allow(dead_code)]

use std::path::PathBuf;

/// Fresh, empty temp dir unique to this test.
pub fn tmp(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("nexus-it-{tag}-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
