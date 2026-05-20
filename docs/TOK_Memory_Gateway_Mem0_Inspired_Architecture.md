# TOK Memory Gateway / Stateful Context Layer
## Cursor-Ready Architecture & Implementation Brief

> Goal: Extend **TOK – Token Optimization Kit** with a local-first, provider-agnostic memory gateway inspired by Mem0’s architecture and lessons, without hard-coupling TOK to Mem0 internals.

> **Rust implementation (shipped):** CLI is `tok memory` (not `tok mem`). Code lives in `src/agent_memory/`; DB is `~/.local/share/tok/memory/tok-memory.db`. Hooks: `tok hook memory-retrieve`, `tok hook memory-extract`. Help: `tok man memory`. See [README.md](../README.md#tok-memory--agent-memory-rules--preferences) and [ARCHITECTURE.md](contributing/ARCHITECTURE.md#agent-memory-gateway).

---

## 1. Executive Summary

TOK currently optimizes and secures prompts before they reach an LLM provider. The next logical feature is a **Stateful Context Layer** that gives stateless LLM calls durable memory.

The system should:

- Remember user preferences, project rules, decisions, facts, and agent/task state.
- Retrieve only relevant memories before each LLM call.
- Inject a compact, token-budgeted memory context block into the prompt.
- Use a local SLM or configured extraction LLM to extract new memory after a conversation.
- Preserve privacy by keeping memory local by default.
- Support multiple memory backends later, including a Mem0-compatible provider.
- Avoid becoming a “memory soup” by using strong scoping, typed memories, provenance, confidence, and retention controls.

This document defines a custom TOK-native memory implementation that borrows heavily from Mem0’s proven design patterns:

- Simple `add` and `search` lifecycle.
- Multi-level memory scoped by user, session, agent, and project.
- Single-pass ADD-style extraction for speed.
- Hybrid retrieval using semantic search, keyword search, entity matching, recency, and confidence.
- Optional reranking.
- Event-log style persistence for auditability.
- Local-first defaults.

---

## 2. Why Build TOK-Native Instead of Directly Embedding Mem0?

Mem0 is an excellent reference architecture, but TOK should not become dependent on one memory engine. TOK’s positioning is broader:

```text
TOK = prompt/security/token/memory gateway for many LLM clients and providers.
Mem0 = memory engine for AI apps/agents.
```

TOK should therefore implement a pluggable memory abstraction:

```text
TOK Memory API
   ├── tok-memory-sqlite       default local implementation
   ├── tok-memory-postgres     team/server mode
   ├── tok-memory-mem0         adapter for existing Mem0 instance
   ├── tok-memory-file         markdown/json memory vault
   └── tok-memory-custom       external provider hook
```

The first implementation should be **TOK-native SQLite + local vector index**, while keeping the interface compatible with Mem0-style `add/search/get/delete` operations.

---

## 3. Core Concepts

### 3.1 Stateless LLM + Stateful Gateway

LLMs remain stateless. TOK becomes the stateful middleware.

```text
User / IDE / CLI / Agent
        ↓
TOK Gateway
        ↓
Memory Preprocessor
        ↓
Prompt Optimizer + Privacy Layer
        ↓
Stateless LLM Provider
        ↓
Response Processor
        ↓
Memory Extractor / Updater
        ↓
Local Memory Store
```

### 3.2 Memory Is Not Chat History

Do not store raw conversations as “memory” and blindly retrieve them. Store structured, typed, scoped memory records.

Memory should answer:

- What should this agent know next time?
- Is this durable?
- Is this current?
- Is this user-approved or inferred?
- Which project/session/agent does this belong to?
- Should this be injected automatically or only retrieved on demand?

---

## 4. Design Principles

1. **Local-first**
   - Default storage should be local under `~/.tok/memory`.
   - Cloud providers should be optional.

2. **Provider-agnostic**
   - Works with OpenAI, Anthropic, Gemini, LM Studio, Ollama, llama.cpp, Cursor, Claude Code, Codex CLI, etc.

3. **Memory is scoped**
   - Every memory belongs to one or more scopes: user, workspace, project, agent, session.

4. **Memory is typed**
   - Rules, preferences, facts, decisions, task state, lessons, summaries, and credentials references must be distinct.

5. **Memory is inspectable**
   - Users must be able to list, view, edit, archive, reject, and forget memory.

6. **Conservative injection**
   - Inject less memory, not more.
   - Bad memory is worse than no memory.

7. **Privacy by default**
   - Never store secrets.
   - Redact sensitive values before storage.
   - Support “memory off” and “private session” modes.

8. **Async where possible**
   - Do not block the main response path on slow memory extraction unless configured.

9. **Composable with TOK’s existing security features**
   - Memory retrieval must run before final prompt construction.
   - Privacy/redaction must be applied before the prompt leaves the machine.

10. **Auditability**
   - Every memory should know why it exists and what source event created it.

---

## 5. High-Level Architecture

```text
┌────────────────────────────────────────────────────────────┐
│ TOK CLI / Proxy / SDK / IDE Hook                           │
└─────────────────────────────┬──────────────────────────────┘
                              ↓
┌────────────────────────────────────────────────────────────┐
│ Request Context Resolver                                   │
│ - userId                                                    │
│ - workspaceId                                               │
│ - projectId                                                 │
│ - agentId                                                   │
│ - sessionId                                                 │
│ - model/provider                                            │
└─────────────────────────────┬──────────────────────────────┘
                              ↓
┌────────────────────────────────────────────────────────────┐
│ Memory Retrieval Layer                                     │
│ - core memory                                               │
│ - project rules                                             │
│ - semantic search                                           │
│ - keyword search                                            │
│ - entity match                                              │
│ - recency/confidence scoring                                │
└─────────────────────────────┬──────────────────────────────┘
                              ↓
┌────────────────────────────────────────────────────────────┐
│ Context Pack Builder                                       │
│ - token budget                                              │
│ - priority sorting                                          │
│ - dedupe                                                    │
│ - contradiction filtering                                   │
│ - memory block formatting                                   │
└─────────────────────────────┬──────────────────────────────┘
                              ↓
┌────────────────────────────────────────────────────────────┐
│ Existing TOK Pipeline                                      │
│ - compression                                               │
│ - prompt optimization                                      │
│ - privacy redaction                                         │
│ - provider routing                                          │
└─────────────────────────────┬──────────────────────────────┘
                              ↓
┌────────────────────────────────────────────────────────────┐
│ Stateless LLM Provider                                     │
│ - OpenAI / Anthropic / Gemini                              │
│ - LM Studio / Ollama / llama.cpp                           │
│ - local CLI adapters                                       │
└─────────────────────────────┬──────────────────────────────┘
                              ↓
┌────────────────────────────────────────────────────────────┐
│ Response Processor                                         │
│ - optional de-redaction                                    │
│ - final response formatting                                │
│ - memory extraction queue                                  │
└─────────────────────────────┬──────────────────────────────┘
                              ↓
┌────────────────────────────────────────────────────────────┐
│ Memory Extraction + Persistence                            │
│ - extract durable facts/rules/preferences                  │
│ - classify memory type                                     │
│ - detect sensitive data                                    │
│ - store event + memory                                     │
│ - embed memory                                             │
│ - update entity index                                      │
└────────────────────────────────────────────────────────────┘
```

---

## 6. Memory Lifecycle

### 6.1 Before LLM Call

```text
1. Receive user prompt.
2. Resolve TOK context:
   - userId
   - projectId
   - workspace path
   - agentId
   - sessionId
   - provider/model
3. Determine if memory is enabled for this call.
4. Retrieve core memories.
5. Retrieve query-relevant memories.
6. Build compact memory context block.
7. Pass enriched prompt through existing TOK optimization/security pipeline.
8. Send request to stateless LLM.
```

### 6.2 After LLM Call

```text
1. Receive LLM response.
2. Return response to user immediately if async memory is enabled.
3. Send interaction to memory extractor:
   - user message
   - assistant response
   - tool events
   - model/provider
   - project/session metadata
4. Extract candidate memories.
5. Filter unsafe/sensitive/low-value memories.
6. Store approved/inferred memories.
7. Embed memories.
8. Update keyword/entity indexes.
9. Log event for audit.
```

---

## 7. Memory Types

```ts
export type TokMemoryType =
  | "identity"
  | "preference"
  | "rule"
  | "project_fact"
  | "decision"
  | "lesson"
  | "task_state"
  | "workflow"
  | "tool_usage"
  | "credential_ref"
  | "conversation_summary"
  | "temporary";
```

### 7.1 Type Behavior

| Type | Inject Automatically? | Scope | Notes |
|---|---:|---|---|
| `identity` | Yes, if relevant | user/agent | Stable identity/persona information |
| `preference` | Yes, if relevant | user/project | Output formats, style, stack preferences |
| `rule` | Yes | user/project/agent | “Always/never” instructions |
| `project_fact` | Query-dependent | project | Architecture decisions, paths, stack |
| `decision` | Query-dependent | project/session | Past decisions and rationale |
| `lesson` | Query-dependent | user/project | Mistakes, fixes, known gotchas |
| `task_state` | Yes, for active session | session/project | Current progress |
| `workflow` | Query-dependent | user/project | Repeatable process memory |
| `tool_usage` | Query-dependent | user/agent | Preferred tools/commands |
| `credential_ref` | Never raw | project/user | Reference only; never store secret values |
| `conversation_summary` | Query-dependent | session/project | Compact summaries |
| `temporary` | Current session only | session | TTL-backed ephemeral memory |

---

## 8. Scoping Model

Use all available dimensions, but never require all of them.

```ts
export interface TokMemoryScope {
  userId: string;
  workspaceId?: string;
  projectId?: string;
  agentId?: string;
  sessionId?: string;
  clientId?: string; // cursor, claude-code, tok-cli, api, etc.
}
```

### 8.1 Scope Priority

When retrieving memory, prioritize in this order:

```text
1. Exact session memory
2. Exact project + agent memory
3. Exact project memory
4. Exact workspace memory
5. User-level rules/preferences
6. Agent-level general memory
7. Global TOK rules
```

---

## 9. Core Data Model

### 9.1 Memory Record

```ts
export interface TokMemoryRecord {
  id: string;
  type: TokMemoryType;

  content: string;
  normalizedContent?: string;

  userId: string;
  workspaceId?: string;
  projectId?: string;
  agentId?: string;
  sessionId?: string;

  source: "user" | "assistant" | "tool" | "system" | "inferred";
  sourceEventId?: string;

  status: "active" | "archived" | "rejected" | "superseded" | "expired";
  confidence: number; // 0-1
  priority: number;   // 0-100

  entities: TokMemoryEntity[];
  tags: string[];

  validFrom?: string;
  validTo?: string;
  expiresAt?: string;

  createdAt: string;
  updatedAt: string;
  lastAccessedAt?: string;

  embeddingId?: string;
  metadata?: Record<string, unknown>;
}
```

### 9.2 Entity Record

```ts
export interface TokMemoryEntity {
  id: string;
  name: string;
  type:
    | "person"
    | "project"
    | "tool"
    | "provider"
    | "model"
    | "file"
    | "repo"
    | "technology"
    | "concept"
    | "organization"
    | "unknown";
  aliases?: string[];
}
```

### 9.3 Event Record

```ts
export interface TokMemoryEvent {
  id: string;
  eventType:
    | "interaction"
    | "memory_added"
    | "memory_rejected"
    | "memory_archived"
    | "memory_accessed"
    | "memory_exported"
    | "memory_deleted";

  userId: string;
  workspaceId?: string;
  projectId?: string;
  agentId?: string;
  sessionId?: string;

  input?: string;
  output?: string;

  provider?: string;
  model?: string;

  createdAt: string;
  metadata?: Record<string, unknown>;
}
```

---

## 10. Storage Architecture

### 10.1 V1 Local Storage

Default path:

```bash
~/.tok/memory/
  tok-memory.db
  vectors/
  exports/
  audit/
```

Use SQLite for structured data.

Recommended packages:

```bash
npm install better-sqlite3
npm install zod
npm install uuid
npm install minisearch
```

For vector search, choose one of:

```text
Option A: LanceDB local
Option B: sqlite-vss / sqlite-vec
Option C: local Qdrant Docker
Option D: simple JSON vector store for MVP only
```

For a Node-first TOK project, the easiest practical first version is:

```text
SQLite + MiniSearch BM25 + pluggable embedding provider
```

Then add vector search behind an interface.

### 10.2 SQLite Schema

```sql
CREATE TABLE IF NOT EXISTS memory_records (
  id TEXT PRIMARY KEY,
  type TEXT NOT NULL,
  content TEXT NOT NULL,
  normalized_content TEXT,

  user_id TEXT NOT NULL,
  workspace_id TEXT,
  project_id TEXT,
  agent_id TEXT,
  session_id TEXT,

  source TEXT NOT NULL,
  source_event_id TEXT,

  status TEXT NOT NULL DEFAULT 'active',
  confidence REAL NOT NULL DEFAULT 0.75,
  priority INTEGER NOT NULL DEFAULT 50,

  entities_json TEXT NOT NULL DEFAULT '[]',
  tags_json TEXT NOT NULL DEFAULT '[]',
  metadata_json TEXT NOT NULL DEFAULT '{}',

  valid_from TEXT,
  valid_to TEXT,
  expires_at TEXT,

  embedding_id TEXT,

  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_accessed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_memory_scope
ON memory_records(user_id, workspace_id, project_id, agent_id, session_id);

CREATE INDEX IF NOT EXISTS idx_memory_type
ON memory_records(type);

CREATE INDEX IF NOT EXISTS idx_memory_status
ON memory_records(status);

CREATE INDEX IF NOT EXISTS idx_memory_created
ON memory_records(created_at);

CREATE TABLE IF NOT EXISTS memory_events (
  id TEXT PRIMARY KEY,
  event_type TEXT NOT NULL,

  user_id TEXT NOT NULL,
  workspace_id TEXT,
  project_id TEXT,
  agent_id TEXT,
  session_id TEXT,

  input TEXT,
  output TEXT,

  provider TEXT,
  model TEXT,

  metadata_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_memory_events_scope
ON memory_events(user_id, workspace_id, project_id, agent_id, session_id);

CREATE TABLE IF NOT EXISTS memory_entities (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  type TEXT NOT NULL,
  aliases_json TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS memory_entity_links (
  memory_id TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  weight REAL NOT NULL DEFAULT 1.0,
  PRIMARY KEY(memory_id, entity_id)
);
```

---

## 11. Provider Interfaces

### 11.1 Main Memory Provider

```ts
export interface TokMemoryProvider {
  add(input: TokMemoryAddInput): Promise<TokMemoryAddResult>;
  search(input: TokMemorySearchInput): Promise<TokMemorySearchResult>;
  get(input: TokMemoryGetInput): Promise<TokMemoryRecord | null>;
  list(input: TokMemoryListInput): Promise<TokMemoryRecord[]>;
  update(input: TokMemoryUpdateInput): Promise<TokMemoryRecord>;
  archive(input: TokMemoryArchiveInput): Promise<void>;
  forget(input: TokMemoryForgetInput): Promise<void>;
  deleteAll(input: TokMemoryDeleteAllInput): Promise<void>;
}
```

### 11.2 Add Input

```ts
export interface TokMemoryAddInput {
  scope: TokMemoryScope;

  messages:
    | string
    | { role: "user" | "assistant" | "system" | "tool"; content: string }
    | Array<{ role: "user" | "assistant" | "system" | "tool"; content: string }>;

  source?: "user" | "assistant" | "tool" | "system" | "inferred";

  mode?: "extract" | "direct";
  typeHint?: TokMemoryType;
  tags?: string[];
  metadata?: Record<string, unknown>;
}
```

### 11.3 Search Input

```ts
export interface TokMemorySearchInput {
  scope: TokMemoryScope;
  query: string;

  filters?: {
    types?: TokMemoryType[];
    tags?: string[];
    status?: TokMemoryRecord["status"][];
    includeExpired?: boolean;
    createdAfter?: string;
    createdBefore?: string;
  };

  topK?: number;
  threshold?: number;
  includeCore?: boolean;
  rerank?: boolean;
}
```

### 11.4 Search Result

```ts
export interface TokMemorySearchResult {
  results: Array<{
    memory: TokMemoryRecord;
    score: number;
    scoreParts: {
      semantic?: number;
      keyword?: number;
      entity?: number;
      recency?: number;
      confidence?: number;
      priority?: number;
    };
    reason?: string;
  }>;
}
```

---

## 12. Hybrid Retrieval

Do not rely on pure vector search. Implement multi-signal retrieval:

```text
final_score =
  semantic_score * 0.45
+ keyword_score  * 0.20
+ entity_score   * 0.15
+ recency_score  * 0.10
+ confidence     * 0.05
+ priority_score * 0.05
```

### 12.1 Retrieval Steps

```text
1. Filter by scope.
2. Filter active memories.
3. Run keyword/BM25 search.
4. Run semantic search if embeddings are enabled.
5. Extract query entities and match against entity index.
6. Merge candidates.
7. Score candidates.
8. Dedupe near-duplicates.
9. Optional rerank with local SLM.
10. Return top K.
```

### 12.2 Core Memory Injection

Some memories should be loaded without semantic search:

```text
- active rules for the project
- active rules for the user
- high-priority preferences
- active task state for current session
```

But still apply token budget limits.

---

## 13. Context Pack Builder

The context pack is the only memory block that reaches the main LLM.

### 13.1 Prompt Format

```md
## TOK Memory Context

The following memory items are relevant to this request. Use them only when applicable. Do not mention them unless useful.

### Active Rules
- [rule:project:95] Always produce Cursor-ready Markdown for implementation briefs.
- [rule:user:90] Prefer local-first architecture unless cloud is explicitly required.

### User Preferences
- [preference:user:88] User prefers implementation-ready specs with TypeScript interfaces and phased plans.

### Project Context
- [project_fact:TOK:85] TOK is a CLI proxy that optimizes prompts and reduces token usage.
- [decision:TOK:80] Memory should be implemented as a pluggable provider layer.

### Current Session
- [task_state:session:75] User is designing a TOK Memory Gateway inspired by Mem0.
```

### 13.2 Token Budget Rules

Configurable defaults:

```yaml
memory:
  context:
    maxTokens: 1200
    maxCoreRules: 8
    maxPreferences: 8
    maxProjectFacts: 10
    maxSessionItems: 10
    maxSearchResults: 12
```

Never exceed `memory.context.maxTokens`.

---

## 14. Memory Extraction

### 14.1 Extraction Strategy

Use single-pass extraction inspired by Mem0’s newer ADD-only approach.

Instead of trying to update/delete existing memories during extraction, extract candidate memories as immutable additions, then handle conflicts with status and retrieval scoring.

```text
Input:
- user message
- assistant response
- optional tool calls
- current scope
- existing relevant memories

Output:
- candidate memory records
```

### 14.2 Extraction Prompt

```md
You are TOK's local memory extractor.

Extract only durable information that will likely be useful in future interactions.

Do not store:
- passwords, API keys, tokens, private keys, seed phrases
- one-time temporary facts
- generic conversation filler
- sensitive personal details unless explicitly requested by the user
- facts that are already represented in existing memory

Return strict JSON only.

Memory types:
- identity
- preference
- rule
- project_fact
- decision
- lesson
- task_state
- workflow
- tool_usage
- credential_ref
- conversation_summary
- temporary

For each memory candidate include:
- type
- content
- confidence: 0 to 1
- priority: 0 to 100
- tags
- entities
- reason
- shouldStore: boolean
- sensitivity: "none" | "low" | "medium" | "high"
- ttlDays: optional number

Rules:
- If the user explicitly says "remember", "from now on", "always", or "never", prefer type "rule" or "preference" with higher confidence.
- If a memory contains a secret value, set shouldStore false.
- If only a reference to a secret location is needed, use credential_ref and never include the secret value.
```

### 14.3 Expected JSON

```json
{
  "memories": [
    {
      "type": "rule",
      "content": "For TOK implementation briefs, produce Cursor-ready Markdown with TypeScript interfaces and phased implementation steps.",
      "confidence": 0.94,
      "priority": 90,
      "tags": ["tok", "cursor", "implementation"],
      "entities": [
        { "name": "TOK", "type": "project" },
        { "name": "Cursor", "type": "tool" }
      ],
      "reason": "The user requested a reusable implementation document for Cursor.",
      "shouldStore": true,
      "sensitivity": "none"
    }
  ]
}
```

---

## 15. Conflict Handling

V1 should not physically delete or mutate old memories automatically. Instead:

```text
- Add new memory.
- Mark obviously conflicting older memory as superseded only when confidence is high.
- Otherwise keep both and rely on recency/confidence/reranking.
```

### 15.1 Supersession Record

Add optional columns later:

```sql
CREATE TABLE IF NOT EXISTS memory_supersessions (
  old_memory_id TEXT NOT NULL,
  new_memory_id TEXT NOT NULL,
  reason TEXT,
  created_at TEXT NOT NULL,
  PRIMARY KEY(old_memory_id, new_memory_id)
);
```

### 15.2 Conflict Prompt

Use local SLM only if needed:

```md
Given a new memory and existing memories, determine whether any existing memory is contradicted or superseded.

Return JSON:
{
  "supersedes": [
    {
      "oldMemoryId": "...",
      "reason": "..."
    }
  ]
}
```

---

## 16. Privacy and Security

This feature expands TOK’s attack surface. Treat memory as sensitive.

### 16.1 Never Store

- API keys
- access tokens
- passwords
- private keys
- seed phrases
- full connection strings with secrets
- exact street addresses unless explicitly requested
- medical/legal/financial sensitive data unless explicitly requested and memory is enabled
- raw files unless explicitly imported

### 16.2 Prompt Injection Defense

Memory should never directly override system/developer policy.

Before injecting memory, sanitize it:

```text
Reject or quarantine memory that says:
- ignore previous instructions
- reveal secrets
- disable security
- exfiltrate memory
- change provider credentials
- run unsafe shell commands
```

### 16.3 Memory Trust Levels

```ts
export type MemoryTrustLevel =
  | "explicit_user"
  | "inferred_user"
  | "assistant_generated"
  | "tool_generated"
  | "imported"
  | "untrusted";
```

Injection priority:

```text
explicit_user > tool_generated > assistant_generated > inferred_user > imported > untrusted
```

Untrusted/imported memories should be used only as context, never as instructions.

---

## 17. TOK Configuration

Add to TOK config:

```yaml
memory:
  enabled: true
  mode: "local" # local | mem0 | server | off
  asyncExtraction: true

  paths:
    root: "~/.tok/memory"
    sqlite: "~/.tok/memory/tok-memory.db"
    vectors: "~/.tok/memory/vectors"

  context:
    maxTokens: 1200
    topK: 12
    threshold: 0.2
    includeCore: true
    rerank: false

  extraction:
    provider: "local" # local | openai | anthropic | lmstudio | ollama
    model: "qwen3-4b-instruct"
    minConfidence: 0.65
    requireExplicitForSensitive: true

  embedding:
    provider: "local" # local | openai | ollama | lmstudio
    model: "nomic-embed-text"
    dimensions: 768

  storage:
    provider: "sqlite" # sqlite | postgres | mem0 | file
    retentionDays: null

  privacy:
    redactBeforeStorage: true
    rejectSecrets: true
    allowSensitiveMemory: false

  scopes:
    defaultUserId: "local-user"
    autoDetectProject: true
    projectMarkers:
      - ".git"
      - "package.json"
      - "tok.config.yaml"
      - ".vibeforge"
```

---

## 18. CLI Commands

Add the following commands:

```bash
tok memory status
tok memory on
tok memory off

tok memory add "User prefers Cursor-ready markdown specs" --type preference --project tok
tok memory search "Cursor markdown specs"
tok memory list
tok memory list --type rule
tok memory list --project tok
tok memory show <memoryId>

tok memory archive <memoryId>
tok memory reject <memoryId>
tok memory forget <memoryId>
tok memory clear --session <sessionId>
tok memory clear --project <projectId>

tok memory export --format json
tok memory export --format markdown
tok memory import ./memory.json

tok memory inspect-context "Build a new TOK plugin"
tok memory compact --project tok
```

### 18.1 `inspect-context`

This is especially useful for debugging:

```bash
tok memory inspect-context "Create a Cursor spec for TOK memory gateway"
```

Output:

```md
# TOK Memory Context Preview

Estimated tokens: 742 / 1200

## Injected Rules
...

## Retrieved Memories
...

## Rejected Candidates
- memory_x rejected because score below threshold
- memory_y rejected because expired
```

---

## 19. Runtime Integration Points

### 19.1 Existing TOK Flow

Current likely flow:

```text
input → optimize → redact → route → provider → restore → output
```

New flow:

```text
input
  → resolve context
  → memory retrieve
  → build context pack
  → optimize
  → redact
  → route
  → provider
  → restore
  → output
  → async memory extraction
```

### 19.2 Middleware Interface

```ts
export interface TokMiddleware {
  name: string;
  beforeRequest?(ctx: TokRequestContext): Promise<TokRequestContext>;
  afterResponse?(ctx: TokResponseContext): Promise<TokResponseContext>;
}
```

Memory as middleware:

```ts
export class TokMemoryMiddleware implements TokMiddleware {
  name = "tok-memory";

  async beforeRequest(ctx: TokRequestContext): Promise<TokRequestContext> {
    if (!ctx.config.memory.enabled) return ctx;

    const memories = await ctx.memory.search({
      scope: ctx.scope,
      query: ctx.input,
      topK: ctx.config.memory.context.topK,
      threshold: ctx.config.memory.context.threshold,
      includeCore: true
    });

    const memoryBlock = await ctx.memoryContextBuilder.build({
      memories,
      maxTokens: ctx.config.memory.context.maxTokens
    });

    return {
      ...ctx,
      systemAdditions: [...ctx.systemAdditions, memoryBlock],
      metadata: {
        ...ctx.metadata,
        memory: {
          injectedCount: memories.results.length,
          estimatedTokens: estimateTokens(memoryBlock)
        }
      }
    };
  }

  async afterResponse(ctx: TokResponseContext): Promise<TokResponseContext> {
    if (!ctx.config.memory.enabled) return ctx;

    const job = {
      scope: ctx.scope,
      messages: [
        { role: "user", content: ctx.originalInput },
        { role: "assistant", content: ctx.output }
      ],
      provider: ctx.provider,
      model: ctx.model
    };

    if (ctx.config.memory.asyncExtraction) {
      ctx.memoryQueue.enqueue(job);
    } else {
      await ctx.memory.add(job);
    }

    return ctx;
  }
}
```

---

## 20. Suggested Folder Structure

```text
src/
  memory/
    index.ts

    types/
      memory.types.ts
      provider.types.ts
      config.types.ts

    providers/
      sqlite/
        SqliteMemoryProvider.ts
        schema.sql
        migrations/
      mem0/
        Mem0MemoryProvider.ts
      file/
        FileMemoryProvider.ts

    retrieval/
      HybridRetriever.ts
      KeywordRetriever.ts
      SemanticRetriever.ts
      EntityRetriever.ts
      Reranker.ts

    extraction/
      MemoryExtractor.ts
      extractionPrompt.ts
      CandidateValidator.ts
      SecretDetector.ts

    context/
      MemoryContextBuilder.ts
      tokenBudget.ts
      formatMemoryBlock.ts

    entities/
      EntityExtractor.ts
      EntityLinker.ts

    middleware/
      TokMemoryMiddleware.ts

    cli/
      memory.commands.ts

    queue/
      MemoryExtractionQueue.ts

    tests/
      memory.add.test.ts
      memory.search.test.ts
      memory.context.test.ts
      memory.security.test.ts
```

---

## 21. Implementation Phases

### Phase 1 — Foundation

Deliverables:

- Config schema for `memory`.
- `TokMemoryProvider` interface.
- SQLite schema and provider.
- CLI commands:
  - `tok memory status`
  - `tok memory add`
  - `tok memory search`
  - `tok memory list`
  - `tok memory forget`
- Manual direct memory add/search.
- Unit tests.

Acceptance criteria:

```text
- User can add a memory manually.
- User can search memory manually.
- Memory is scoped by user/project/session.
- Memory can be deleted.
- No LLM extraction required yet.
```

---

### Phase 2 — Pre-Request Context Injection

Deliverables:

- Context resolver.
- Core memory retrieval.
- Keyword search using MiniSearch or SQLite FTS.
- Context pack builder.
- Middleware integration before provider call.
- `tok memory inspect-context`.

Acceptance criteria:

```text
- TOK injects relevant rules/preferences into LLM prompt.
- Token budget is respected.
- User can preview exactly what memory would be injected.
- Memory can be disabled per command.
```

Example:

```bash
tok run "Create a Cursor implementation plan" --memory
tok run "Create a Cursor implementation plan" --no-memory
```

---

### Phase 3 — Automatic Memory Extraction

Deliverables:

- Local extraction provider interface.
- Extraction prompt.
- Candidate validation.
- Secret detection.
- Async extraction queue.
- Event logging.

Acceptance criteria:

```text
- After a conversation, TOK extracts durable candidate memories.
- Secrets are rejected.
- Low-confidence memories are rejected or marked for review.
- Extracted memories appear in `tok memory list`.
```

---

### Phase 4 — Hybrid Retrieval

Deliverables:

- Embedding provider interface.
- Local embedding provider support.
- Vector index integration.
- Entity extraction.
- Entity matching.
- Weighted retrieval scoring.
- Optional reranker.

Acceptance criteria:

```text
- Search uses keyword + semantic + entity + recency scoring.
- Results include score breakdown.
- Relevant memories are found even when wording differs.
```

---

### Phase 5 — Memory Review and Governance

Deliverables:

- Review queue.
- `tok memory review`.
- Archive/reject/supersede workflows.
- Markdown export.
- JSON import/export.
- Audit log viewer.

Acceptance criteria:

```text
- User can inspect inferred memories before trusting them.
- User can archive/reject bad memories.
- User can export all memory.
- User can clear memory by project/session/user.
```

---

### Phase 6 — Mem0 Adapter

Deliverables:

- `Mem0MemoryProvider`.
- Config support for external Mem0.
- API mapping:
  - TOK `add` → Mem0 `add`
  - TOK `search` → Mem0 `search`
  - TOK `list/get/delete` → Mem0 equivalent where available
- Compatibility tests.

Acceptance criteria:

```text
- TOK can use local SQLite provider by default.
- TOK can switch to Mem0 provider by config.
- TOK internal interfaces do not change.
```

Example config:

```yaml
memory:
  enabled: true
  mode: "mem0"
  storage:
    provider: "mem0"
  mem0:
    baseUrl: "http://localhost:8888"
    apiKeyEnv: "MEM0_API_KEY"
```

---

## 22. Testing Strategy

### 22.1 Unit Tests

```text
- Adds memory with correct scope.
- Searches only within allowed scope.
- Does not return archived memory.
- Rejects secrets.
- Respects token budget.
- Formats context block correctly.
- Scores memory deterministically.
```

### 22.2 Integration Tests

```text
- Full request → retrieval → injection → response → extraction flow.
- Async extraction does not block response.
- Memory off mode performs no reads/writes.
- Project-scoped memory does not leak across projects.
```

### 22.3 Security Tests

```text
- Memory containing “ignore previous instructions” is quarantined.
- API keys are detected and rejected.
- Prompt injection in stored memory is not injected as instruction.
- Imported memory is marked untrusted.
```

---

## 23. Example End-to-End Flow

### User Prompt

```text
From now on, when we create TOK specs, give me one single downloadable markdown file that is Cursor-ready.
```

### Extraction Result

```json
{
  "memories": [
    {
      "type": "rule",
      "content": "When creating TOK specs, produce one single downloadable Cursor-ready Markdown file.",
      "confidence": 0.97,
      "priority": 95,
      "tags": ["tok", "cursor", "markdown"],
      "entities": [
        { "name": "TOK", "type": "project" },
        { "name": "Cursor", "type": "tool" }
      ],
      "shouldStore": true,
      "sensitivity": "none"
    }
  ]
}
```

### Future Prompt

```text
Create the architecture brief for the memory gateway.
```

### Injected Memory

```md
## TOK Memory Context

### Active Rules
- [rule:project:95] When creating TOK specs, produce one single downloadable Cursor-ready Markdown file.
```

---

## 24. Cursor Implementation Prompt

Use this prompt inside Cursor:

```md
You are working inside the TOK - Token Optimization Kit codebase.

Implement the TOK Memory Gateway / Stateful Context Layer described in this document.

Primary objectives:
1. Add a pluggable memory provider architecture.
2. Implement a local SQLite-backed memory provider first.
3. Add CLI commands for manual memory add/search/list/forget/status.
4. Add request middleware that retrieves scoped memory and injects a compact memory block into the TOK prompt pipeline.
5. Add post-response memory extraction as a later phase, behind a feature flag.
6. Keep the design compatible with a future Mem0 provider adapter.

Important constraints:
- Do not hard-code Mem0 as the only backend.
- Do not store secrets or sensitive values.
- Memory must be scoped by user/project/session/agent where possible.
- Memory injection must respect a token budget.
- Memory should be inspectable with a CLI command.
- Keep existing TOK behavior unchanged when memory.enabled=false.
- Add tests for all new provider and middleware behavior.
- Prefer TypeScript interfaces, Zod config validation, and clean provider boundaries.

Start with Phase 1 and Phase 2 only unless the codebase already has suitable LLM extraction infrastructure.
```

---

## 25. Recommended Defaults for TOK

```yaml
memory:
  enabled: false
  mode: "local"
  asyncExtraction: true

  context:
    maxTokens: 900
    topK: 8
    threshold: 0.25
    includeCore: true
    rerank: false

  extraction:
    enabled: false
    provider: "local"
    model: "qwen3-4b-instruct"
    minConfidence: 0.70

  privacy:
    redactBeforeStorage: true
    rejectSecrets: true
    allowSensitiveMemory: false
```

Reasoning:

- Keep memory off by default until stable.
- Let users enable it explicitly.
- Keep extraction disabled until manual memory and context injection are reliable.
- Start with small context budget to protect TOK’s token-saving purpose.

---

## 26. Key Lessons Borrowed from Mem0

1. Use a simple developer-facing API:
   - `add`
   - `search`
   - `get`
   - `delete/forget`

2. Support user/session/agent memory scopes.

3. Treat memory as a first-class layer, not just RAG.

4. Use hybrid retrieval, not only vector search.

5. Prefer single-pass extraction for latency and cost.

6. Keep local/self-hosted operation possible.

7. Make the storage/provider layer configurable.

8. Evaluate memory quality using real conversations and retrieval tests.

---

## 27. Non-Goals for V1

Do not implement these in the first pass:

- Full graph database.
- Automatic contradiction reasoning across all memory.
- Web dashboard.
- Multi-user server auth.
- Cloud sync.
- Advanced reranking.
- Fine-tuning.
- Browser extension.
- Complex autonomous agents.

These can come later once the local core works.

---

## 28. Success Criteria

The feature is successful when:

```text
- TOK can remember user/project rules without resending them manually.
- Memory remains local by default.
- Prompt injection still works with existing TOK optimization and security.
- Memory reduces repeated context while preserving output quality.
- The user can inspect and delete memories.
- The memory provider can later be swapped for Mem0 or another backend.
```

---

## 29. Future Enhancements

```text
V2:
- embedding-backed semantic search
- local SLM extraction
- review queue
- import/export memory vault

V3:
- Mem0 adapter
- Graphiti-style temporal graph provider
- project memory dashboards
- IDE sidebar integration

V4:
- multi-agent shared memory
- agent-specific procedural memory
- memory quality evaluation harness
- secure sync between machines
```

---

## 30. Final Recommendation

Build TOK Memory Gateway as a **pluggable, local-first, Mem0-inspired memory system**, not as a direct Mem0 clone.

Best first implementation:

```text
SQLite
+ typed memory records
+ scoped retrieval
+ keyword search
+ compact context injection
+ memory CLI
+ strict privacy filters
```

Then add:

```text
local embeddings
+ hybrid scoring
+ SLM extraction
+ Mem0 adapter
+ temporal/project graph memory
```

This gives TOK a powerful stateful context layer while keeping its original mission intact: smaller, safer, smarter LLM calls across any provider.
