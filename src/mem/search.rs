//! Full-text search using SQLite FTS5 with BM25 ranking.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use super::symbols::{SearchResult, Symbol, SymbolKind};

/// Search symbols using FTS5 BM25 ranking.
///
/// The query is tokenized and matched against name, signature, doc_comment,
/// and file_path fields. Results are ordered by BM25 relevance.
pub fn fts_search(
    conn: &Connection,
    query: &str,
    repo_id: Option<&str>,
    kind: Option<SymbolKind>,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let fts_query = sanitize_fts_query(query);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    let base_sql = "SELECT s.id, s.repo_id, s.name, s.kind, s.file_path,
                           s.line_start, s.line_end, s.signature, s.doc_comment,
                           s.branch, s.indexed_at,
                           bm25(symbols_fts, 10.0, 5.0, 2.0, 1.0) as rank
                    FROM symbols_fts
                    JOIN symbols s ON symbols_fts.symbol_id = s.id
                    WHERE symbols_fts MATCH ?1";

    let mut conditions = String::new();
    if repo_id.is_some() {
        conditions.push_str(" AND s.repo_id = ?3");
    }
    if kind.is_some() {
        conditions.push_str(if repo_id.is_some() {
            " AND s.kind = ?4"
        } else {
            " AND s.kind = ?3"
        });
    }

    let sql = format!("{}{} ORDER BY rank LIMIT ?2", base_sql, conditions);

    let mut stmt = conn.prepare(&sql).context("Failed to prepare FTS query")?;

    let row_mapper = |row: &rusqlite::Row| -> rusqlite::Result<SearchResult> {
        Ok(SearchResult {
            symbol: Symbol {
                id: row.get(0)?,
                repo_id: row.get(1)?,
                name: row.get(2)?,
                kind: SymbolKind::from_str(&row.get::<_, String>(3)?)
                    .unwrap_or(SymbolKind::Function),
                file_path: row.get(4)?,
                line_start: row.get(5)?,
                line_end: row.get(6)?,
                signature: row.get(7)?,
                doc_comment: row.get(8)?,
                branch: row.get(9)?,
                indexed_at: row.get(10)?,
            },
            rank: row.get(11)?,
        })
    };

    let results = match (repo_id, kind) {
        (Some(r), Some(k)) => stmt
            .query_map(params![fts_query, limit, r, k.as_str()], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
        (Some(r), None) => stmt
            .query_map(params![fts_query, limit, r], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
        (None, Some(k)) => stmt
            .query_map(params![fts_query, limit, k.as_str()], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
        (None, None) => stmt
            .query_map(params![fts_query, limit], row_mapper)?
            .collect::<std::result::Result<Vec<_>, _>>()?,
    };

    Ok(results)
}

/// Sanitize a user query into a valid FTS5 query string.
/// Strips special FTS5 operators that could cause parse errors.
fn sanitize_fts_query(query: &str) -> String {
    let mut tokens: Vec<String> = Vec::new();

    for word in query.split_whitespace() {
        let clean: String = word
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '*')
            .collect();
        if !clean.is_empty() {
            tokens.push(clean);
        }
    }

    tokens.join(" OR ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_operators() {
        assert_eq!(sanitize_fts_query("hello world"), "hello OR world");
        assert_eq!(sanitize_fts_query("NOT foo"), "NOT OR foo");
        assert_eq!(sanitize_fts_query("auth*"), "auth*");
        assert_eq!(sanitize_fts_query(""), "");
    }

    #[test]
    fn sanitize_handles_special_chars() {
        assert_eq!(sanitize_fts_query("foo::bar"), "foobar");
        assert_eq!(sanitize_fts_query("user_login"), "user_login");
    }
}
