//! Shared helpers that used to be copied file by file (review 2026-11-13 §2.4, package 2).
//!
//! - [`http`]: one reqwest client builder; every setting is named by the caller.
//! - [`sqlite`]: one opener for the app's own sqlite stores (private files, WAL, busy timeout).
//! - [`task`]: one `blocking()` for running sync work from an async command.
//! - [`fsx`]: one atomic write (unique temp file, fsync, rename, no symlink replace).
//! - [`time`]: one place for the JST offset, epoch clocks and the local day key.
//!
//! Nothing here changes what a caller does: each helper takes the settings, mode and error text the
//! caller already had. The NOOP reader (`noop.rs::open_store`) keeps its own read-only opener.

pub(crate) mod fsx;
pub(crate) mod http;
pub(crate) mod sqlite;
pub(crate) mod task;
pub(crate) mod time;
