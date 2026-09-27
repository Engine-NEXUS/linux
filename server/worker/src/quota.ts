/**
 * Per-user quota tracking and cost control.
 *
 * Tracks daily usage in D1 (usage_log table) and enforces per-user limits.
 * When the global neuron budget approaches the 10K/day free allocation,
 * switches to the cheapest model for non-analysis requests.
 */

// ---- Per-user daily limits (configurable) ----
// Tuned for 10 users × 150 requests/day on Workers Paid ($5/mo) plan.
// PR analysis uses GLM-4.7-flash (Cloudflare neurons).
// Architecture + research routed to free external providers (Gemini/Groq).
export const LIMITS = {
  requests_per_day: 150,        // 50 PR + 50 research + 50 architecture
  ai_neurons_per_day: 8000,     // PR analysis only (arch/research go external)
  deep_calls_per_day: 15,       // GLM-5.3-flash deep reviews
  search_calls_per_day: 50,     // Wikipedia/Wikidata + Gemini synthesis
};

// Global neuron budget — 10K free/day on Workers Paid, then $0.011/1K neurons.
// PR analysis is the only neuron consumer now (architecture + research are free).
// Warn at 8K (switch to cheaper model), hard reject deep at 9.5K.
const GLOBAL_NEURON_WARN = 8000; // switch to cheap model
const GLOBAL_NEURON_HARD = 9500; // reject deep analysis

function todayUTC(): string {
  return new Date().toISOString().slice(0, 10);
}

export interface UsageRow {
  requests: number;
  ai_neurons: number;
  d1_reads: number;
  d1_writes: number;
  search_calls: number;
  deep_calls: number;
}

/**
 * Get the current day's usage for a user. Returns zeros if no row exists.
 */
export async function getUsage(env: Env, userId: string): Promise<UsageRow> {
  const day = todayUTC();
  const row = await env.DB.prepare(
    "SELECT requests, ai_neurons, d1_reads, d1_writes, search_calls, deep_calls FROM usage_log WHERE user_id = ? AND day_utc = ?"
  ).bind(userId, day).first();

  if (!row) {
    return { requests: 0, ai_neurons: 0, d1_reads: 0, d1_writes: 0, search_calls: 0, deep_calls: 0 };
  }
  return {
    requests: (row.requests as number) || 0,
    ai_neurons: (row.ai_neurons as number) || 0,
    d1_reads: (row.d1_reads as number) || 0,
    d1_writes: (row.d1_writes as number) || 0,
    search_calls: (row.search_calls as number) || 0,
    deep_calls: (row.deep_calls as number) || 0,
  };
}

/**
 * Get the global (all-users combined) neuron usage for today.
 */
export async function getGlobalNeurons(env: Env): Promise<number> {
  const day = todayUTC();
  const row = await env.DB.prepare(
    "SELECT COALESCE(SUM(ai_neurons), 0) as total FROM usage_log WHERE day_utc = ?"
  ).bind(day).first();
  return (row?.total as number) || 0;
}

/**
 * Increment usage counters for a user. Uses INSERT OR REPLACE to upsert.
 */
export async function incrementUsage(
  env: Env,
  userId: string,
  delta: Partial<UsageRow>
): Promise<void> {
  const day = todayUTC();
  const current = await getUsage(env, userId);

  const updated: UsageRow = {
    requests: current.requests + (delta.requests || 0),
    ai_neurons: current.ai_neurons + (delta.ai_neurons || 0),
    d1_reads: current.d1_reads + (delta.d1_reads || 0),
    d1_writes: current.d1_writes + (delta.d1_writes || 0),
    search_calls: current.search_calls + (delta.search_calls || 0),
    deep_calls: current.deep_calls + (delta.deep_calls || 0),
  };

  await env.DB.prepare(
    "INSERT OR REPLACE INTO usage_log (user_id, day_utc, requests, ai_neurons, d1_reads, d1_writes, search_calls, deep_calls) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
  ).bind(
    userId, day,
    updated.requests, updated.ai_neurons, updated.d1_reads,
    updated.d1_writes, updated.search_calls, updated.deep_calls
  ).run();
}

export interface QuotaCheckResult {
  allowed: boolean;
  reason?: string;
  shouldDegradeModel: boolean;  // true if global neurons approaching limit
  remaining: UsageRow;
}

/**
 * Check if a request is allowed under per-user and global quotas.
 */
export async function checkQuota(
  env: Env,
  userId: string,
  isDeep: boolean,
  isSearch: boolean
): Promise<QuotaCheckResult> {
  const usage = await getUsage(env, userId);
  const globalNeurons = await getGlobalNeurons(env);

  // Per-user request limit
  if (usage.requests >= LIMITS.requests_per_day) {
    return {
      allowed: false,
      reason: `Daily request limit (${LIMITS.requests_per_day}) reached. Try again tomorrow.`,
      shouldDegradeModel: false,
      remaining: usage,
    };
  }

  // Per-user neuron limit
  if (usage.ai_neurons >= LIMITS.ai_neurons_per_day) {
    return {
      allowed: false,
      reason: `Daily AI usage limit reached. Try again tomorrow.`,
      shouldDegradeModel: false,
      remaining: usage,
    };
  }

  // Deep analysis limit
  if (isDeep && usage.deep_calls >= LIMITS.deep_calls_per_day) {
    return {
      allowed: false,
      reason: `Daily deep analysis limit (${LIMITS.deep_calls_per_day}) reached. Use 'analyse' without 'deep' for faster results.`,
      shouldDegradeModel: false,
      remaining: usage,
    };
  }

  // Search limit
  if (isSearch && usage.search_calls >= LIMITS.search_calls_per_day) {
    return {
      allowed: false,
      reason: `Daily search limit (${LIMITS.search_calls_per_day}) reached.`,
      shouldDegradeModel: false,
      remaining: usage,
    };
  }

  // Global neuron budget — degrade but don't reject
  const shouldDegrade = globalNeurons >= GLOBAL_NEURON_WARN;

  // Global hard limit for deep analysis
  if (isDeep && globalNeurons >= GLOBAL_NEURON_HARD) {
    return {
      allowed: false,
      reason: `Global AI budget for today is exhausted. Deep analysis unavailable until midnight UTC.`,
      shouldDegradeModel: true,
      remaining: usage,
    };
  }

  return {
    allowed: true,
    shouldDegradeModel: shouldDegrade,
    remaining: usage,
  };
}

// ---- Env type re-export (shared across modules) ----
export interface Env {
  AI: Ai;
  DB: D1Database;
  CACHE?: KVNamespace;
  GOOGLE_CLIENT_ID: string;
  GOOGLE_CLIENT_SECRET: string;
  GITHUB_CLIENT_ID: string;
  GITHUB_CLIENT_SECRET: string;
  NEXUS_ENCRYPTION_KEY: string;
  // Research API keys (Cloudflare secrets)
  TAVILY_API_KEY?: string;
  SEARCHX_API_KEY?: string;
  WOLFRAM_API_KEY?: string;
  SEMANTIC_SCHOLAR_API_KEY?: string;
  SERPER_API_KEY?: string;
  GOOGLE_CSE_API_KEY?: string;
  GOOGLE_CSE_CX?: string; // Custom Search Engine ID
  // External LLM API keys (Cloudflare secrets)
  GEMINI_API_KEY?: string;
  GROQ_API_KEY?: string;
}
