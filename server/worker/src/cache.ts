/**
 * Edge caching helpers.
 *
 * Uses KV namespace (if bound) or falls back to D1 cache_entries table.
 * All cache keys are namespaced per-user to prevent cross-user leakage.
 * Public repo metadata uses a shared prefix for cross-user dedup.
 */

import type { Env } from "./quota";

// ---- Key namespacing ----

export function userKey(userId: string, key: string): string {
  return `u:${userId}:${key}`;
}

export function pubKey(key: string): string {
  return `pub:${key}`;
}

export function searchKey(lang: string, query: string): string {
  // Simple hash for query (avoid storing raw PII in cache keys)
  let hash = 0;
  const s = `${lang}:${query.toLowerCase()}`;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    hash = ((hash << 5) - hash) + c;
    hash |= 0;
  }
  return `search:${lang}:${Math.abs(hash).toString(36)}`;
}

export function prAnalysisKey(userId: string, repo: string, prNumber: number, contextHash: string): string {
  return userKey(userId, `pr:${repo}:${prNumber}:v${contextHash}`);
}

export function repoMetaKey(owner: string, repo: string): string {
  return pubKey(`repo:${owner}:${repo}:meta`);
}

// Simple content hash (FNV-1a, fast and sufficient for cache keys)
export function contentHash(content: string): string {
  let hash = 2166136261;
  for (let i = 0; i < content.length; i++) {
    hash ^= content.charCodeAt(i);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(36);
}

// ---- Cache read ----

export async function cacheGet<T>(env: Env, key: string, ttlSeconds: number = 3600): Promise<T | null> {
  // Try KV first (if bound)
  if (env.CACHE) {
    try {
      const raw = await env.CACHE.get(key, "json");
      if (raw) return raw as T;
    } catch {
      // KV miss or error, fall through to D1
    }
  }

  // Fall back to D1 cache_entries
  const now = Date.now() / 1000;
  const row = await env.DB.prepare(
    "SELECT cache_value, expires_at FROM cache_entries WHERE cache_key = ?"
  ).bind(key).first();

  if (!row) return null;
  if ((row.expires_at as number) < now) {
    // Expired — delete and return null
    await env.DB.prepare("DELETE FROM cache_entries WHERE cache_key = ?").bind(key).run();
    return null;
  }

  try {
    return JSON.parse(row.cache_value as string) as T;
  } catch {
    return null;
  }
}

// ---- Cache write ----

export async function cacheSet(env: Env, key: string, value: unknown, ttlSeconds: number = 3600): Promise<void> {
  const raw = JSON.stringify(value);

  // Try KV first
  if (env.CACHE) {
    try {
      await env.CACHE.put(key, raw, { expirationTtl: ttlSeconds });
      return;
    } catch {
      // Fall through to D1
    }
  }

  // Fall back to D1
  const now = Date.now() / 1000;
  const expiresAt = now + ttlSeconds;
  await env.DB.prepare(
    "INSERT OR REPLACE INTO cache_entries (cache_key, cache_value, expires_at, created_at) VALUES (?, ?, ?, ?)"
  ).bind(key, raw, expiresAt, now).run();
}
// NOTE: cacheDelete was deleted (audit M4) — defined but never imported.
