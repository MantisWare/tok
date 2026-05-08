//! Optional SLM (Small Language Model) integration via embedded llama.cpp.
//! Disabled by default. Provides semantic entity detection beyond regex patterns.

pub mod binary_resolver;
pub mod doctor;
pub mod model_resolver;
pub mod prompts;
pub mod runtime;
