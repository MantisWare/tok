//! `tok memory` command handlers.

use std::collections::HashMap;
use std::io::{self, Write};

use anyhow::{bail, Context, Result};
use colored::Colorize;

use crate::agent_memory::privacy::{check_content, reason_message};
use crate::agent_memory::provider::TokMemoryProvider;
use crate::agent_memory::service::{
    build_context_pack, memory_config, open_provider, resolve_current_scope,
    set_extraction_enabled, set_memory_enabled,
};
use crate::agent_memory::sqlite::db;
use crate::agent_memory::types::{
    DeleteMode, MemorySource, MemoryStatus, TokMemoryAddInput, TokMemoryListInput,
    TokMemorySearchInput, TokMemoryType,
};
use crate::MemoryCommands;

pub fn dispatch(cmd: MemoryCommands) -> Result<i32> {
    match cmd {
        MemoryCommands::Status => run_status(),
        MemoryCommands::On => {
            set_memory_enabled(true)?;
            println!("{} Agent memory enabled", "✓".green());
            Ok(0)
        }
        MemoryCommands::Off => {
            set_memory_enabled(false)?;
            println!("{} Agent memory disabled", "✓".green());
            Ok(0)
        }
        MemoryCommands::Extraction { enabled } => {
            set_extraction_enabled(enabled)?;
            println!(
                "{} Auto-extraction {}",
                "✓".green(),
                if enabled { "enabled" } else { "disabled" }
            );
            Ok(0)
        }
        MemoryCommands::Add {
            content,
            memory_type,
            project,
            session,
            tags,
        } => run_add(
            &content,
            &memory_type,
            project.as_deref(),
            session.as_deref(),
            tags,
        ),
        MemoryCommands::Search {
            query,
            project,
            verbose,
        } => run_search(&query, project.as_deref(), verbose),
        MemoryCommands::List {
            memory_type,
            project,
            status,
            limit,
        } => run_list(
            memory_type.as_deref(),
            project.as_deref(),
            status.as_deref(),
            limit,
        ),
        MemoryCommands::Show { id } => run_show(&id),
        MemoryCommands::Forget { id } => run_forget(&id),
        MemoryCommands::Archive { id } => run_archive(&id),
        MemoryCommands::Reject { id } => run_reject(&id),
        MemoryCommands::InspectContext { query, json } => run_inspect_context(&query, json),
        MemoryCommands::ContextPack { query, json } => run_context_pack(query.as_deref(), json),
        MemoryCommands::Clear { session, project } => run_clear(session, project),
        MemoryCommands::Export { format, output } => run_export(&format, output.as_deref()),
        MemoryCommands::Import { path } => run_import(&path),
        MemoryCommands::Events { limit } => run_events(limit),
        MemoryCommands::Review { limit } => run_review(limit),
    }
}

fn run_status() -> Result<i32> {
    let cfg = memory_config();
    let path = db::db_path()?;
    let provider = open_provider()?;
    let counts = provider.status_counts()?;

    println!("{}", "TOK Agent Memory".bold());
    println!("  Enabled:     {}", cfg.enabled);
    println!("  Extraction:  {}", cfg.extraction.enabled);
    println!("  Database:    {}", path.display());
    println!("  Total:       {}", counts.total);
    println!("  Active:      {}", counts.active);
    println!("  Archived:    {}", counts.archived);
    println!("  Rejected:    {}", counts.rejected);
    Ok(0)
}

fn run_add(
    content: &str,
    type_str: &str,
    project: Option<&str>,
    session: Option<&str>,
    tags: Vec<String>,
) -> Result<i32> {
    let cfg = memory_config();
    if let Some(reason) = check_content(content, cfg.privacy.reject_secrets) {
        bail!("refused to store memory: {}", reason_message(reason));
    }

    let memory_type = TokMemoryType::from_str(type_str)
        .with_context(|| format!("unknown memory type: {type_str}"))?;

    let mut scope = resolve_current_scope();
    if let Some(p) = project {
        scope.project_id = Some(p.to_string());
    }
    if let Some(s) = session {
        scope.session_id = Some(s.to_string());
    }

    let provider = open_provider()?;
    let result = provider.add(&TokMemoryAddInput {
        scope,
        content: content.to_string(),
        memory_type,
        source: MemorySource::User,
        confidence: 0.95,
        priority: 80,
        tags,
        metadata: HashMap::new(),
    })?;

    if result.created {
        println!("{} Stored memory {}", "✓".green(), result.id.bold());
    } else {
        println!("{} Duplicate memory exists: {}", "·".yellow(), result.id);
    }
    Ok(0)
}

fn run_search(query: &str, project: Option<&str>, verbose: bool) -> Result<i32> {
    let cfg = memory_config();
    let mut scope = resolve_current_scope();
    if let Some(p) = project {
        scope.project_id = Some(p.to_string());
    }

    let provider = open_provider()?;
    let results = provider.search(&TokMemorySearchInput {
        scope,
        query: query.to_string(),
        types: None,
        top_k: cfg.context.top_k,
        threshold: cfg.context.threshold,
        include_core: false,
    })?;

    if results.is_empty() {
        println!("No matching memories.");
        return Ok(0);
    }

    for item in results {
        println!(
            "[{:.2}] {} ({})",
            item.score,
            item.memory.id.bold(),
            item.memory.memory_type.as_str()
        );
        println!("  {}", item.memory.content);
        if verbose {
            if let Some(k) = item.score_parts.keyword {
                println!("    keyword={k:.2}");
            }
            if let Some(r) = item.score_parts.recency {
                println!("    recency={r:.2}");
            }
        }
    }
    Ok(0)
}

fn run_list(
    memory_type: Option<&str>,
    project: Option<&str>,
    status: Option<&str>,
    limit: usize,
) -> Result<i32> {
    let mut scope = resolve_current_scope();
    if let Some(p) = project {
        scope.project_id = Some(p.to_string());
    }
    let ty = memory_type
        .map(|s| TokMemoryType::from_str(s).context("invalid --type"))
        .transpose()?;
    let st = status
        .map(|s| MemoryStatus::from_str(s).context("invalid --status"))
        .transpose()?;

    let provider = open_provider()?;
    let records = provider.list(&TokMemoryListInput {
        scope,
        memory_type: ty,
        status: st,
        limit,
    })?;

    if records.is_empty() {
        println!("No memories found.");
        return Ok(0);
    }

    for r in records {
        println!(
            "{} [{}] {} — {}",
            r.id.dimmed(),
            r.memory_type.as_str(),
            r.status.as_str(),
            r.content
        );
    }
    Ok(0)
}

fn run_show(id: &str) -> Result<i32> {
    let provider = open_provider()?;
    let Some(r) = provider.get(id)? else {
        bail!("memory not found: {id}");
    };
    println!("id:       {}", r.id);
    println!("type:     {}", r.memory_type.as_str());
    println!("status:   {}", r.status.as_str());
    println!("source:   {}", r.source.as_str());
    println!(
        "score:    confidence={} priority={}",
        r.confidence, r.priority
    );
    println!("scope:    user={}", r.user_id);
    if let Some(p) = &r.project_id {
        println!("          project={p}");
    }
    if let Some(s) = &r.session_id {
        println!("          session={s}");
    }
    println!("content:  {}", r.content);
    Ok(0)
}

fn run_forget(id: &str) -> Result<i32> {
    let provider = open_provider()?;
    provider.forget(id)?;
    println!("{} Forgot {}", "✓".green(), id.bold());
    Ok(0)
}

fn run_archive(id: &str) -> Result<i32> {
    let provider = open_provider()?;
    provider.archive(id)?;
    println!("{} Archived {}", "✓".green(), id.bold());
    Ok(0)
}

fn run_reject(id: &str) -> Result<i32> {
    let conn = db::open()?;
    let now = chrono::Utc::now().to_rfc3339();
    let n = conn.execute(
        "UPDATE memory_records SET status = 'rejected', updated_at = ?2 WHERE id = ?1",
        rusqlite::params![id, now],
    )?;
    if n == 0 {
        bail!("memory not found: {id}");
    }
    println!("{} Rejected {}", "✓".green(), id.bold());
    Ok(0)
}

fn run_inspect_context(query: &str, json: bool) -> Result<i32> {
    let pack = build_context_pack(query)?;
    if json {
        let out = serde_json::json!({
            "markdown": pack.markdown,
            "estimated_tokens": pack.estimated_tokens,
            "max_tokens": memory_config().context.max_tokens,
            "injected_count": pack.injected_count,
            "rejected": pack.rejected,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("# TOK Memory Context Preview\n");
        println!(
            "Estimated tokens: {} / {}",
            pack.estimated_tokens,
            memory_config().context.max_tokens
        );
        println!();
        if pack.markdown.is_empty() {
            println!("(empty — no memories injected)");
        } else {
            println!("{}", pack.markdown);
        }
        if !pack.rejected.is_empty() {
            println!("\n## Rejected Candidates");
            for r in &pack.rejected {
                println!("- {r}");
            }
        }
    }
    Ok(0)
}

fn run_context_pack(query: Option<&str>, json: bool) -> Result<i32> {
    let q = query.unwrap_or("");
    let pack = build_context_pack(q)?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "additional_context": pack.markdown,
                "estimated_tokens": pack.estimated_tokens,
                "injected_count": pack.injected_count,
            }))?
        );
    } else {
        print!("{}", pack.markdown);
        io::stdout().flush()?;
    }
    Ok(0)
}

fn run_clear(session: bool, project: bool) -> Result<i32> {
    let scope = resolve_current_scope();
    let provider = open_provider()?;
    let (mode, label) = if session {
        (DeleteMode::Session, "session")
    } else if project {
        (DeleteMode::Project, "project")
    } else {
        bail!("specify --session or --project");
    };
    let n = provider.delete_all(&scope, mode)?;
    println!("{} Cleared {n} memories for {label}", "✓".green());
    Ok(0)
}

fn run_export(format: &str, output: Option<&str>) -> Result<i32> {
    let provider = open_provider()?;
    let records = provider.list(&TokMemoryListInput {
        scope: resolve_current_scope(),
        memory_type: None,
        status: None,
        limit: 10_000,
    })?;

    let content = match format {
        "json" => serde_json::to_string_pretty(&records)?,
        "markdown" => {
            let mut md = String::from("# TOK Memory Export\n\n");
            for r in records {
                md.push_str(&format!(
                    "## {} ({:?})\n\n{}\n\n",
                    r.id, r.memory_type, r.content
                ));
            }
            md
        }
        _ => bail!("unsupported format: {format} (use json or markdown)"),
    };

    if let Some(path) = output {
        std::fs::write(path, content)?;
        println!("{} Exported to {}", "✓".green(), path);
    } else {
        print!("{content}");
    }
    Ok(0)
}

fn run_import(path: &str) -> Result<i32> {
    let data = std::fs::read_to_string(path)?;
    let records: Vec<crate::agent_memory::types::TokMemoryRecord> = serde_json::from_str(&data)?;
    let provider = open_provider()?;
    let scope = resolve_current_scope();
    let mut n = 0u64;
    for r in records {
        if provider
            .add(&TokMemoryAddInput {
                scope: scope.clone(),
                content: r.content,
                memory_type: r.memory_type,
                source: MemorySource::User,
                confidence: r.confidence,
                priority: r.priority,
                tags: serde_json::from_str(&r.tags_json).unwrap_or_default(),
                metadata: HashMap::new(),
            })?
            .created
        {
            n += 1;
        }
    }
    println!("{} Imported {n} new memories", "✓".green());
    Ok(0)
}

fn run_events(limit: usize) -> Result<i32> {
    let conn = db::open()?;
    let scope = resolve_current_scope();
    let mut stmt = conn.prepare(
        "SELECT id, event_type, created_at, metadata_json FROM memory_events
         WHERE user_id = ?1 ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![scope.user_id, limit as i64], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (id, ty, at, meta) = row?;
        println!("{at} [{ty}] {id} {meta}");
    }
    Ok(0)
}

fn run_review(limit: usize) -> Result<i32> {
    let provider = open_provider()?;
    let records = provider.list(&TokMemoryListInput {
        scope: resolve_current_scope(),
        memory_type: None,
        status: Some(MemoryStatus::Active),
        limit,
    })?;
    for r in records {
        if r.source == MemorySource::Inferred {
            println!("{} [{}] {}", r.id, r.memory_type.as_str(), r.content);
        }
    }
    Ok(0)
}
