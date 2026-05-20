//! Future TOK-native storage backends (file vault, postgres).
//!
//! V1 uses SQLite only via [`super::sqlite::SqliteMemoryProvider`].
//! No external memory engines (e.g. Mem0) are integrated.
