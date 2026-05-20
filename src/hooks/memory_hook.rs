//! Hook entry points for automatic agent memory retrieve / extract.

use std::io::{self, Read, Write};

use anyhow::{Context, Result};
use serde_json::Value;

use super::memory_payload::{
    apply_scope_from_input, cache_prompt_from_input, extract_turn_from_input,
    format_empty_retrieve, format_retrieve_output, MemoryHookAgent, MemoryHookEvent,
};
use crate::agent_memory::service::{build_context_pack, enqueue_extraction, is_enabled};

/// `tok hook memory-retrieve` — JSON for session / per-turn hooks.
pub fn run_memory_retrieve(
    query: Option<&str>,
    json: bool,
    agent: Option<&str>,
    event: Option<&str>,
    use_stdin: bool,
) -> Result<()> {
    let agent = MemoryHookAgent::parse(agent).resolve();
    let event = MemoryHookEvent::parse(event);

    let mut hook_input: Option<Value> = None;
    if use_stdin {
        let mut raw = String::new();
        io::stdin().read_to_string(&mut raw)?;
        if !raw.trim().is_empty() {
            let v: Value =
                serde_json::from_str(&raw).context("parse memory-retrieve hook stdin JSON")?;
            apply_scope_from_input(&v, agent);
            hook_input = Some(v);
        }
    }

    if !is_enabled() {
        if json {
            println!("{}", serde_json::to_string(&format_empty_retrieve(agent))?);
        }
        return Ok(());
    }

    let q = query
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            hook_input
                .as_ref()
                .map(|v| super::memory_payload::retrieve_query_from_input(v, event))
        })
        .unwrap_or_default();

    let pack = build_context_pack(&q)?;

    if json {
        let out = format_retrieve_output(
            agent,
            event,
            &pack.markdown,
            pack.estimated_tokens,
            pack.injected_count,
        );
        println!("{}", serde_json::to_string(&out)?);
    } else {
        print!("{}", pack.markdown);
        io::stdout().flush()?;
    }
    Ok(())
}

/// `tok hook memory-cache-prompt` — store user prompt for Cursor turn pairing.
pub fn run_memory_cache_prompt(agent: Option<&str>, use_stdin: bool) -> Result<()> {
    if !use_stdin {
        return Ok(());
    }
    let agent = MemoryHookAgent::parse(agent).resolve();
    let mut raw = String::new();
    io::stdin().read_to_string(&mut raw)?;
    if raw.trim().is_empty() {
        return Ok(());
    }
    let v: Value = serde_json::from_str(&raw).context("parse memory-cache-prompt stdin JSON")?;
    apply_scope_from_input(&v, agent);
    cache_prompt_from_input(&v)
}

/// `tok hook memory-extract` — enqueue extraction from completed turn JSON on stdin.
pub fn run_memory_extract(agent: Option<&str>, use_stdin: bool) -> Result<()> {
    if !is_enabled() {
        return Ok(());
    }

    let agent_parsed = MemoryHookAgent::parse(agent).resolve();

    let (user, assistant) = if use_stdin {
        let mut raw = String::new();
        io::stdin().read_to_string(&mut raw)?;
        if raw.trim().is_empty() {
            (String::new(), String::new())
        } else {
            let v: Value =
                serde_json::from_str(&raw).context("parse memory-extract hook stdin JSON")?;
            apply_scope_from_input(&v, agent_parsed);
            extract_turn_from_input(&v, agent_parsed)
        }
    } else {
        (String::new(), String::new())
    };

    if user.is_empty() && assistant.is_empty() {
        return Ok(());
    }

    enqueue_extraction(user, assistant);
    Ok(())
}
