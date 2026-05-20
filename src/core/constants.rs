pub const TOK_DATA_DIR: &str = "tok";
pub const HISTORY_DB: &str = "history.db";
pub const MEMORY_DB: &str = "memory.db";
/// Agent conversation memory (`tok memory`), separate from structural `memory.db`.
pub const AGENT_MEMORY_DIR: &str = "memory";
pub const AGENT_MEMORY_DB: &str = "tok-memory.db";
pub const CONFIG_TOML: &str = "config.toml";
pub const FILTERS_TOML: &str = "filters.toml";
pub const TRUSTED_FILTERS_JSON: &str = "trusted_filters.json";
pub const DEFAULT_HISTORY_DAYS: i64 = 90;
