//! SLM prompt templates for security scanning and validation.

/// System prompt for the security scanner SLM.
pub const SCANNER_SYSTEM_PROMPT: &str = r#"You are TOK Security SLM.

Your task is to inspect user prompts for sensitive data that deterministic scanners may miss.

Return JSON only.

Do not include markdown.
Do not include explanations outside JSON.
Do not rewrite the original prompt.
Do not include chain-of-thought."#;

/// Build the user prompt for scanning.
pub fn scanner_user_prompt(text: &str) -> String {
    format!(
        r#"Analyze the following prompt for sensitive entities.

Return this JSON schema:

{{
  "sensitive_entities": [
    {{
      "type": "person | company | client | internal_project | medical | legal | custom",
      "value": "exact text span from prompt",
      "confidence": 0.0,
      "recommended_action": "allow | placeholder",
      "reason": "short reason"
    }}
  ],
  "risk_level": "none | low | medium | high | critical",
  "safe_to_send": true,
  "warnings": []
}}

Prompt:
---
{}
---"#,
        text
    )
}

/// System prompt for restoration validation.
pub const VALIDATION_SYSTEM_PROMPT: &str = r#"You are TOK Restoration Validator.

You inspect a restored LLM response and verify whether any TOK placeholders or fake replacement values remain unresolved.

Return JSON only."#;

/// Build the user prompt for restoration validation.
pub fn validation_user_prompt(text: &str) -> String {
    format!(
        r#"Check if this response has any remaining TOK placeholders (like {{{{TOK_EMAIL_001}}}}) or unresolved values.

Return:
{{
  "restoration_status": "complete | incomplete",
  "unresolved_placeholders": [],
  "warnings": []
}}

Response:
---
{}
---"#,
        text
    )
}
