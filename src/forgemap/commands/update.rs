//! `tok forgemap update` — annotate only files missing a header.
//!
//! Thin wrapper: calls `run_init` with `force: false`.

use anyhow::Result;

use crate::forgemap::types::{InitOptions, InitResult};

use super::init::run_init;

/// Run update: same as init but never force-replaces existing headers.
pub fn run_update(opts: &InitOptions) -> Result<InitResult> {
    let mut safe_opts = opts.clone();
    safe_opts.force = false;
    run_init(&safe_opts)
}
