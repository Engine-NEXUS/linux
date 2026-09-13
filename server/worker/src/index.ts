/**
 * NEXUS Cloudflare Worker — fully serverless. No sidecar, no server needed.
 *
 * Handles everything:
 *   - User/device registration (POST /api/register)
 *   - OAuth authorization URL generation (GET /oauth/auth-url)
 *   - OAuth code exchange (POST /oauth/exchange)
 *   - OAuth status (GET /oauth/status)
 *   - OAuth disconnect (DELETE /oauth/disconnect)
 *   - API key management (POST /apikeys/add, DELETE /apikeys/remove, GET /apikeys/list)
 *   - Transcript processing (POST /) — classify intent, call APIs, summarize
 *   - Health check (GET /health)
 *
 * Storage: Cloudflare D1 (free SQLite, 5GB)
 * AI: Cloudflare Workers AI (free, 10K neurons/day)
 *
 * Deploy:
 *   cd server/worker
 *   npx wrangler d1 create nexus-db          # creates the database
 *   npx wrangler d1 execute nexus-db --file=schema.sql  --remote  # creates tables
 *   npx wrangler secret put GOOGLE_CLIENT_ID
 *   npx wrangler secret put GOOGLE_CLIENT_SECRET
 *   npx wrangler secret put GITHUB_CLIENT_ID
 *   npx wrangler secret put GITHUB_CLIENT_SECRET
 *   npx wrangler secret put NEXUS_ENCRYPTION_KEY  # Fernet key for API keys
 *   npx wrangler deploy
 */

// ---- Module imports ----
import { checkQuota, incrementUsage, getUsage, type UsageRow } from "./quota";
import { cacheGet, cacheSet, contentHash, prAnalysisKey, searchKey } from "./cache";
import { retrieve, retrieveCascade, buildSearchSynthesisPrompt, isSearchQuestion, searchWikipedia, type SearchResult } from "./research";
import { synthesizeWithCascade, callGroq, callGemini } from "./external_llm";
import { dedupeSources, stripInjection, buildResponse, extractCaveats } from "./clean";
import {
  ANALYSIS_FALLBACK_CHAIN, DEEP_ANALYSIS_FALLBACK_CHAIN, SUMMARY_FALLBACK_CHAIN,
  SEARCH_SYNTHESIS_FALLBACK_CHAIN, CHEAP_FALLBACK_MODEL,
  selectAnalysisModel, selectSearchModel, selectSummaryModel,
  truncateContext, FLASH_CONTEXT_LIMIT_CHARS,
} from "./models";

// ---- Types ----

interface Env {
  AI: Ai;
  DB: D1Database;
  CACHE?: KVNamespace;  // optional KV namespace for edge caching
  // Secrets (set via wrangler secret put)
  GOOGLE_CLIENT_ID: string;
  GOOGLE_CLIENT_SECRET: string;
  GITHUB_CLIENT_ID: string;
  GITHUB_CLIENT_SECRET: string;
  NEXUS_ENCRYPTION_KEY: string;
}

// ---- OAuth configuration ----

const GOOGLE_TOKEN_URL = "https://oauth2.googleapis.com/token";
const GITHUB_TOKEN_URL = "https://github.com/login/oauth/access_token";
const OAUTH_REDIRECT_URI = "nexus://oauth/callback";

const GOOGLE_SCOPES = [
  "https://www.googleapis.com/auth/gmail.readonly",
  "https://www.googleapis.com/auth/calendar",
  "https://www.googleapis.com/auth/drive.readonly",
  "openid",
  "email",
  "profile",
].join(" ");

const GITHUB_SCOPES = "repo read:org workflow";

// ---- Model constants (re-exported from models.ts for backward compat) ----
const INTENT_MODEL = "@cf/meta/llama-3.2-1b-instruct";
const SUMMARY_MODEL = "@cf/mistral/mistral-small-3.1-24b-instruct";
const SMALL_SUMMARY_MODEL = "@cf/meta/llama-3.2-3b-instruct";
const ANALYSIS_MODEL = "@cf/zai-org/glm-4.7-flash";
const DEEP_ANALYSIS_MODEL = "@cf/zai-org/glm-5.3-flash";

// Track recent analyses to detect re-evaluation requests
// Maps "user_id:repo:prNumber" → timestamp of last analysis
const recentAnalyses = new Map<string, number>();
const RE_EVALUATION_WINDOW_MS = 5 * 60 * 1000;  // 5 minutes

/**
 * Extract text from any Workers AI model response format.
 * - Older models (llama, mistral): { response: "text" }
 * - OpenAI-compatible (GLM-5.2, GLM-5.3-Flash): { choices: [{ message: { content: "text" } }] }
 * - Reasoning models (GLM-4.7-Flash): content may be null, reasoning_content has the thinking
 *   → if content is null, fall back to reasoning_content
 */
function extractText(response: any): string {
  if (response?.response) return response.response.trim();
  const msg = response?.choices?.[0]?.message;
  if (msg?.content) return msg.content.trim();
  // Reasoning model fallback: if content is null, use reasoning_content
  if (msg?.reasoning_content) return msg.reasoning_content.trim();
  if (response?.choices?.[0]?.text) return response.choices[0].text.trim();
  if (response?.result?.response) return response.result.response.trim();
  return "";
}

/**
 * Detect greetings, thanks, and identity questions so the LLM classifier
 * doesn't misclassify them as "search" (e.g., "who are you" → LLM says search).
 */
function isGreetingOrThanks(transcript: string): boolean {
  const t = transcript.toLowerCase().trim();
  return /\b(hello|hi|hey|thanks|thank you|good morning|good evening|good afternoon|how are you|who are you|what can you do|what is your name|bye|goodbye|see you|never mind)\b/.test(t);
}

async function classifyIntent(transcript: string, env: Env): Promise<string> {
  // Check keyword fallback FIRST for reliable intent detection.
  // The LLM classifier is a secondary signal — keywords are more reliable
  // for the analyse vs. check distinction.
  const keywordIntent = keywordFallback(transcript);
  if (keywordIntent !== "general") {
    return keywordIntent;
  }

  // Greetings/thanks were matched by keywordFallback as "general" — but we
  // don't want the LLM classifier to override them with "search" (e.g.,
  // "who are you" → LLM says "search"). Short-circuit here.
  if (isGreetingOrThanks(transcript)) {
    return "general";
  }

  const prompt = `You are an intent classifier. Read the user request and respond with exactly one word from this list:
- github (for GitHub PRs, issues, repos, code)
- gmail (for email, inbox, messages)
- calendar (for schedule, meetings, events, appointments)
- search (for web searches, looking up information)
- general (for anything else)

User request: "${transcript}"

Intent:`;

  try {
    const response = await env.AI.run(INTENT_MODEL as any, {
      messages: [{ role: "user", content: prompt }],
      max_tokens: 5,
    });
    const text = extractText(response).toLowerCase();
    const word = text.split(/\s+/)[0].replace(/[^a-z]/g, "");
    if (["github", "gmail", "calendar", "search", "general"].includes(word)) return word;
    return "general";
  } catch {
    return "general";
  }
}

function keywordFallback(transcript: string): string {
  const t = transcript.toLowerCase();

  // Fast repo analyse — "analyse owner/repo", "analyze owner/repo"
  // This is DIFFERENT from the architecture mapper (which uses "analyze THIS repo")
  // and from PR analysis (which uses "analyse PR #123").
  // Pattern: "deep analyse <owner>/<repo>" or "deep analysis <owner>/<repo>"
  // Also matches "deep scan"
  // Must NOT match: "analyse this repo", "analyse PR", "analyse branch"
  if (/\b(deep\s+analy[sz]e|deep\s+analy[sz]is|deep\s+scan)\b/.test(t)
      && /\b([a-z0-9_.\-]+\/[a-z0-9_.\-]+|repo|repository)\b/.test(t)) {
    return "deep_analyse";
  }

  if (/\b(analy[sz]e|analy[sz]is)\b/.test(t)
      && !/\b(this|that|the)\s+repo\b/.test(t)
      && !/\bpr\s*#?\s*\d+\b/.test(t)
      && !/\bbranch\b/.test(t)
      && !/\bpull\s*request\b/.test(t)
      && !/\barchitecture\b/.test(t)
      && /\b([a-z0-9_.\-]+\/[a-z0-9_.\-]+|[a-z0-9_\-]+)\b/.test(t)) {
    // Make sure it's not "analyse the codebase" or similar
    if (!/\b(codebase|project|architecture|dependencies|dependency)\b/.test(t)) {
      return "fast_analyse";
    }
  }

  // Architecture Mapper intent — e.g. "analyze this repo", "map the codebase",
  // "create architecture", "build architecture", "show architecture"
  if (/\b(analy[sz]e|map|understand|explore|scan|visuali[sz]e|create|build|show|generate|make)\b/.test(t)
      && /\b(repo|repository|codebase|project|architecture|dependencies|dependency)\b/.test(t)) {
    return "analyze_repo";
  }
  // Also catch "architecture" alone or "architecture in <repo>" patterns
  if (/\barchitecture\b/.test(t) && /\b(in|of|for|from)\b/.test(t)) {
    return "analyze_repo";
  }
  if (/\b(what breaks|blast radius|impact analysis|consequence)\b/.test(t)) {
    return "analyze_repo";
  }

  // GitHub write operations — MUST be checked BEFORE github_analyse
  // because "merge PR", "close PR", "approve PR" contain keywords that
  // would otherwise match the analyse intent.
  if (/\b(merge|approve|close|comment)\b/.test(t)
      && /\b(pr|pull\s*request|issue)\b/.test(t)) {
    return "github_write";
  }
  if (/\b(create|open)\b/.test(t)
      && /\b(pr|pull\s*request|issue)\b/.test(t)
      && !/\b(analy[sz]e|review|deep\s*dive)\b/.test(t)) {
    return "github_write";
  }

  // Deep analysis intent — keywords like "analyse", "analyze", "review", "deep dive"
  // Must match BEFORE the generic github check
  if (/\b(analy[sz]e|analy[sz]ing|analy[sz]is|review|deep\s*dive|critique|evaluate|assess|inspect|examine|take\s+another\s+look|look\s+at)\b/.test(t)
      && /\b(pr|pull\s*request|repo|repository|code|commit|branch|merge|github|diff|patch|change)\b/.test(t)) {
    return "github_analyse";
  }

  // Also catch "analyse the PR" / "review the PR" / "analysis of the PR" even without explicit github keyword
  if (/\b(analy[sz]e|analy[sz]is|review|deep\s*dive|critique|evaluate|take\s+another\s+look)\b/.test(t)
      && /\b(pr|pull\s*request)\b/.test(t)) {
    return "github_analyse";
  }

  // STT often mishears "analyse" as "unless", "analyze" (without the 's'), etc.
  // If the transcript contains "PR <number>" + "in/on <something>", treat it as
  // github_analyse — the user clearly wants a PR analysis.
  // Also catches "PR number 24 on NEXUS agent" (STT inserts "number")
  if (/\bpr\s*(?:number|#\s*)?\s*#?\s*\d+\b/.test(t) && /\b(in|of|from|on)\b/.test(t)) {
    return "github_analyse";
  }
  // "analyse PR number 24 on NEXUS agent" — analyse + PR + number
  if (/\b(analy[sz]e|review)\b/.test(t) && /\bpr\s*(?:number|#\s*)?\s*#?\s*\d+\b/.test(t)) {
    return "github_analyse";
  }

  if (/\b(pr|pull requests?|repo|repository|commit|issue|branch|merge|github|list\s+prs)\b/.test(t)) return "github";

  if (/\b(email|inbox|mail|message|gmail|send to)\b/.test(t)) return "gmail";
  if (/\b(calendar|schedule|meeting|event|appointment)\b/.test(t)) return "calendar";
  // Greetings / thanks — must be checked BEFORE search so "thank you nexus"
  // doesn't get misclassified as a search question by the LLM.
  if (/\b(hello|hi|hey|thanks|thank you|thank you nexus|good morning|good evening|good afternoon|how are you|who are you|what can you do|what is your name|bye|goodbye|see you|never mind)\b/.test(t)) return "general";
  if (/\b(search|google|look up|find|what is|who is|where is|research|look\s*up|tell me about|explain|define)\b/.test(t)) return "search";
  return "general";
}

async function summarize(prompt: string, env: Env, useLarge = true): Promise<string> {
  const model = useLarge ? SUMMARY_MODEL : SMALL_SUMMARY_MODEL;
  try {
    const response = await env.AI.run(model as any, {
      messages: [{ role: "user", content: prompt }],
      max_tokens: 300,
    });
    return extractText(response) || "I couldn't summarize that.";
  } catch {
    if (useLarge) return summarize(prompt, env, false);
    return "I couldn't process that request.";
  }
}

// ---- D1 credential helpers ----

/**
 * Convert a GitHub API error status into a user-friendly spoken message.
 * 401 → "token revoked/expired, reconnect GitHub"
 * 403 → "permission denied, check OAuth scopes"
 * 404 → "not found"
 * other → generic error
 */
function githubErrorMessage(status: number, context: string): string {
  if (status === 401) {
    return `Your GitHub token has expired or been revoked, sir. Please reconnect GitHub in the NEXUS setup wizard to ${context}.`;
  }
  if (status === 403) {
    return `GitHub denied permission for that action, sir. Your OAuth scopes may not include the required access. Needed: ${context}.`;
  }
  if (status === 404) {
    return `I couldn't find that on GitHub, sir. It may not exist or you don't have access.`;
  }
  return `GitHub API error: ${status}`;
}

async function getCredentialsFromD1(env: Env, userId: string): Promise<{
  google?: { access_token: string; scopes: string; refresh_token?: string; expires_at?: number };
  github?: { access_token: string };
}> {
  const result = await env.DB.prepare(
    "SELECT provider, access_token, refresh_token, expires_at, scopes FROM oauth_tokens WHERE user_id = ?"
  ).bind(userId).all();

  const creds: Record<string, any> = {};
  for (const row of result.results || []) {
    creds[row.provider as string] = {
      access_token: row.access_token as string,
      refresh_token: row.refresh_token as string | undefined,
      expires_at: row.expires_at as number | undefined,
      scopes: row.scopes as string | undefined,
    };
  }
  return creds;
}

async function refreshGoogleToken(env: Env, refreshToken: string): Promise<{ access_token: string; expires_in: number }> {
  const resp = await fetch(GOOGLE_TOKEN_URL, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      client_id: env.GOOGLE_CLIENT_ID,
      client_secret: env.GOOGLE_CLIENT_SECRET,
      refresh_token: refreshToken,
      grant_type: "refresh_token",
    }),
  });
  if (!resp.ok) throw new Error(`Google refresh failed: ${resp.status}`);
  const data = await resp.json() as any;
  return { access_token: data.access_token, expires_in: data.expires_in || 3600 };
}

async function getValidGoogleToken(env: Env, userId: string): Promise<string | null> {
  const row = await env.DB.prepare(
    "SELECT access_token, refresh_token, expires_at FROM oauth_tokens WHERE user_id = ? AND provider = 'google'"
  ).bind(userId).first();

  if (!row) return null;

  const now = Date.now() / 1000;
  const expiresAt = row.expires_at as number;

  // Refresh if expired (with 60s buffer) and we have a refresh token
  if (expiresAt && now > expiresAt - 60 && row.refresh_token) {
    try {
      const refreshed = await refreshGoogleToken(env, row.refresh_token as string);
      const newExpiresAt = now + refreshed.expires_in;
      await env.DB.prepare(
        "UPDATE oauth_tokens SET access_token = ?, expires_at = ? WHERE user_id = ? AND provider = 'google'"
      ).bind(refreshed.access_token, newExpiresAt, userId).run();
      return refreshed.access_token;
    } catch {
      // Fall back to the stored token (might still work briefly)
      return row.access_token as string;
    }
  }

  return row.access_token as string;
}

async function getValidGithubToken(env: Env, userId: string): Promise<string | null> {
  const row = await env.DB.prepare(
    "SELECT access_token, refresh_token, expires_at FROM oauth_tokens WHERE user_id = ? AND provider = 'github'"
  ).bind(userId).first();
  if (!row) return null;

  const now = Date.now() / 1000;
  const expiresAt = row.expires_at as number;

  // If expires_at is 0, this is a classic OAuth App token (never expires)
  // Just return it as-is.
  if (!expiresAt) {
    return row.access_token as string;
  }

  // GitHub App expiring token — refresh if within 5 minutes of expiry
  if (now > expiresAt - 300 && row.refresh_token) {
    try {
      const refreshed = await refreshGithubToken(env, row.refresh_token as string);
      const newExpiresAt = now + refreshed.expires_in;
      await env.DB.prepare(
        "UPDATE oauth_tokens SET access_token = ?, refresh_token = ?, expires_at = ? WHERE user_id = ? AND provider = 'github'"
      ).bind(refreshed.access_token, refreshed.refresh_token, newExpiresAt, userId).run();
      console.log(`GitHub token refreshed for user ${userId}`);
      return refreshed.access_token;
    } catch (err) {
      // Refresh failed — return old token (might still work briefly)
      console.error(`GitHub refresh failed: ${(err as Error).message}`);
      return row.access_token as string;
    }
  }

  return row.access_token as string;
}

/**
 * Refresh a GitHub App expiring token.
 * Only works with GitHub Apps that have expiring tokens enabled.
 * Classic OAuth App tokens don't have refresh tokens.
 */
async function refreshGithubToken(env: Env, refreshToken: string): Promise<{
  access_token: string;
  refresh_token: string;
  expires_in: number;
}> {
  const resp = await fetch(GITHUB_TOKEN_URL, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "Accept": "application/json",
    },
    body: JSON.stringify({
      client_id: env.GITHUB_CLIENT_ID,
      client_secret: env.GITHUB_CLIENT_SECRET,
      grant_type: "refresh_token",
      refresh_token: refreshToken,
    }),
  });

  if (!resp.ok) {
    const body = await resp.text();
    throw new Error(`GitHub refresh failed: ${resp.status} ${body}`);
  }

  const data = await resp.json() as any;
  if (!data.access_token) {
    throw new Error(`GitHub refresh returned no token: ${JSON.stringify(data)}`);
  }

  return {
    access_token: data.access_token,
    refresh_token: data.refresh_token,
    expires_in: data.expires_in || 28800, // GitHub App tokens default to 8 hours
  };
}

// ---- GitHub handler ----

async function handleGitHub(req: NexusRequest, env: Env, token: string): Promise<string> {
  const transcriptLower = req.task.request.toLowerCase();
  const transcriptOrig = req.task.request;  // preserve case for repo names
  const headers: Record<string, string> = {
    "Authorization": `Bearer ${token}`,
    "Accept": "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
    "User-Agent": "NEXUS-Worker",
  };

  // Match on lowercased transcript for keywords, but extract repo from original
  const prMatch = transcriptLower.match(/(?:pr|pull request)\s*#?\s*(\d+)\s*(?:of|in|from)?\s*(?:repo\s+)?([\w\-./]+)?/);
  const listPrMatch = transcriptLower.match(/(?:list|show|open)\s+(?:open\s+)?(?:prs|pull requests?)(?:\s+(?:in|of|from)\s+([\w\-./]+))?/);
  // "latest PR", "current PR", "newest PR", "most recent PR", "check PR",
  // "view PR", "see PR", "get PR" — fetch the most recent PR (open or all).
  // The optional "in/of/from <repo>" group captures the repo.
  const latestPrMatch = transcriptLower.match(/(?:check|view|see|get|show|look\s+at|latest|current|newest|most\s+recent|recent|last)\s+(?:the\s+)?(?:latest\s+|current\s+|newest\s+|most\s+recent\s+)?(?:pr|pull\s*request)(?:\s+(?:in|of|from)\s+([\w\-./]+))?/);
  const issueMatch = transcriptLower.match(/(?:issue|bug)\s*#?\s*(\d+)\s*(?:in|of|from)?\s*(?:repo\s+)?([\w\-./]+)?/);

  // Extract repo name from the original transcript (preserves case).
  // groupIdx: which capture group in the regex holds the repo name.
  // prMatch uses group 2 (pr number is group 1), listPrMatch uses group 1.
  function extractRepo(lowerMatch: RegExpMatchArray | null, groupIdx: number = 2): string | null {
    if (!lowerMatch || !lowerMatch[groupIdx]) return null;
    // Find the repo name in the original transcript at the same position
    const repoLower = lowerMatch[groupIdx];
    const idx = transcriptLower.indexOf(repoLower);
    if (idx >= 0) return transcriptOrig.substr(idx, repoLower.length);
    return repoLower;
  }

  try {
    if (prMatch) {
      const prNum = prMatch[1];
      let repo = extractRepo(prMatch) || "zync";
      if (!repo.includes("/")) {
        // Try to resolve via user's repos
        const resolved = await resolveRepo(token, repo);
        if (resolved.full_name) repo = resolved.full_name;
      }

      const resp = await fetch(`https://api.github.com/repos/${repo}/pulls/${prNum}`, { headers });
      if (!resp.ok) return githubErrorMessage(resp.status, `read PR #${prNum} in ${repo}`);
      const pr = await resp.json() as Record<string, unknown>;
      const prInfo = `PR #${pr["number"]}: ${pr["title"]}
State: ${pr["state"]}, Mergeable: ${pr["mergeable_state"] || "unknown"}
Author: ${(pr["user"] as Record<string, string>)?.login || "unknown"}
Body: ${(pr["body"] as string || "").slice(0, 500)}
Changes: +${pr["additions"]} -${pr["deletions"]} across ${pr["changed_files"]} files`;

      return await summarize(
        `Summarize this GitHub PR for the user in 2-3 sentences. Be concise and mention the status, what it changes, and whether it's ready to merge:\n\n${prInfo}`,
        env
      );
    }

    if (listPrMatch) {
      let repo = extractRepo(listPrMatch, 1) || "zync";
      if (!repo.includes("/")) {
        const resolved = await resolveRepo(token, repo);
        if (resolved.full_name) repo = resolved.full_name;
      }

      const resp = await fetch(`https://api.github.com/repos/${repo}/pulls?state=open&per_page=10`, { headers });
      if (!resp.ok) return githubErrorMessage(resp.status, `list PRs in ${repo}`);
      const prs = await resp.json() as Array<Record<string, unknown>>;
      if (prs.length === 0) return `There are no open pull requests in ${repo}.`;
      const prList = prs.map((pr, i) =>
        `${i + 1}. PR #${pr["number"]}: ${pr["title"]} (by ${(pr["user"] as Record<string, string>)?.login})`
      ).join("\n");

      return await summarize(
        `The user asked for open PRs in ${repo}. Summarize this list concisely:\n\n${prList}`,
        env
      );
    }

    if (latestPrMatch) {
      // Fetch the most recent PR. Default to open state, but if none are open,
      // fall back to the most recent PR of any state so the user still gets an answer.
      let repo = extractRepo(latestPrMatch, 1) || "zync";
      if (!repo.includes("/")) {
        const resolved = await resolveRepo(token, repo);
        if (resolved.full_name) repo = resolved.full_name;
      }

      let resp = await fetch(`https://api.github.com/repos/${repo}/pulls?state=open&sort=created&direction=desc&per_page=1`, { headers });
      let prs = resp.ok ? (await resp.json() as Array<Record<string, unknown>>) : [];
      if (prs.length === 0) {
        // No open PRs — try the most recent PR of any state
        resp = await fetch(`https://api.github.com/repos/${repo}/pulls?state=all&sort=created&direction=desc&per_page=1`, { headers });
        prs = resp.ok ? (await resp.json() as Array<Record<string, unknown>>) : [];
      }
      if (!resp.ok) return githubErrorMessage(resp.status, `fetch latest PR in ${repo}`);
      if (prs.length === 0) return `There are no pull requests in ${repo}.`;
      const pr = prs[0];
      const prInfo = `PR #${pr["number"]}: ${pr["title"]}
State: ${pr["state"]}, Mergeable: ${pr["mergeable_state"] || "unknown"}
Author: ${(pr["user"] as Record<string, string>)?.login || "unknown"}
Body: ${(pr["body"] as string || "").slice(0, 500)}
Changes: +${pr["additions"]} -${pr["deletions"]} across ${pr["changed_files"]} files`;

      return await summarize(
        `Summarize this GitHub PR for the user in 2-3 sentences. Be concise and mention the status, what it changes, and whether it's ready to merge:\n\n${prInfo}`,
        env
      );
    }

    if (issueMatch) {
      const issueNum = issueMatch[1];
      let repo = extractRepo(issueMatch) || "zync";
      if (!repo.includes("/")) {
        const resolved = await resolveRepo(token, repo);
        if (resolved.full_name) repo = resolved.full_name;
      }

      const resp = await fetch(`https://api.github.com/repos/${repo}/issues/${issueNum}`, { headers });
      if (!resp.ok) return githubErrorMessage(resp.status, `read issue #${issueNum} in ${repo}`);
      const issue = await resp.json() as Record<string, unknown>;
      const issueInfo = `Issue #${issue["number"]}: ${issue["title"]}
State: ${issue["state"]}
Author: ${(issue["user"] as Record<string, string>)?.login || "unknown"}
Body: ${(issue["body"] as string || "").slice(0, 500)}`;

      return await summarize(`Summarize this GitHub issue in 2-3 sentences:\n\n${issueInfo}`, env);
    }

    return await summarize(
      `The user asked: "${req.task.request}". This is a GitHub-related request but I couldn't parse a specific PR or issue number. Suggest how they might phrase it, e.g., "check PR 24 in owner/repo".`,
      env, false
    );
  } catch (err) {
    return `I had trouble reaching GitHub. Error: ${(err as Error).message}`;
  }
}

// ---- GitHub Write Operations handler ----
//
// Handles: create PR, merge PR, close PR, approve PR, comment on PR,
//          close issue, create issue.
// Destructive operations (merge, close) include a confirmation step.

async function handleGitHubWrite(req: NexusRequest, env: Env, token: string): Promise<string> {
  const transcript = req.task.request;
  const t = transcript.toLowerCase();
  const headers: Record<string, string> = {
    "Authorization": `Bearer ${token}`,
    "Accept": "application/vnd.github+json",
    "User-Agent": "NEXUS-Worker",
    "X-GitHub-Api-Version": "2022-11-28",
  };

  // Parse repo name from transcript
  let repo: string | null = null;
  const repoMatch = transcript.match(/(?:in|from|on|of)\s+([a-zA-Z0-9_.\-]+\/[a-zA-Z0-9_.\-]+)/i);
  if (repoMatch) {
    repo = repoMatch[1];
  } else {
    const repoMatch2 = transcript.match(/(?:in|from|on|of)\s+([a-zA-Z0-9_.\-]+)/i);
    if (repoMatch2) {
      const resolved = await resolveRepo(token, repoMatch2[1]);
      repo = resolved.full_name;
    }
  }

  // ─── MERGE PR ────────────────────────────────────────────────────
  // "merge PR #123", "merge pull request 123 in owner/repo"
  if (/\bmerge\b/.test(t) && /\bpr\s*#?\s*\d+\b/.test(t)) {
    if (!repo) return "Which repository is this PR in? Say something like 'merge PR 123 in owner/repo'.";
    const prNum = (t.match(/\bpr\s*#?\s*(\d+)\b/) || [])[1];
    if (!prNum) return "Which PR number should I merge?";

    // Check if confirmation is in the transcript
    const isConfirmed = /\b(yes|confirm|do it|go ahead|proceed)\b/.test(t);

    if (!isConfirmed) {
      // First pass — ask for confirmation
      return `Are you sure you want to merge PR #${prNum} in ${repo}? Say "yes merge PR ${prNum} in ${repo}" to confirm.`;
    }

    // Confirmed — merge
    try {
      const resp = await fetch(`https://api.github.com/repos/${repo}/pulls/${prNum}`, { headers });
      if (!resp.ok) return githubErrorMessage(resp.status, `merge PR #${prNum} in ${repo}`);
      const pr = await resp.json() as any;
      if (!pr.mergeable) return `PR #${prNum} is not mergeable. It may have conflicts.`;

      const mergeResp = await fetch(`https://api.github.com/repos/${repo}/pulls/${prNum}/merge`, {
        method: "PUT",
        headers: { ...headers, "Content-Type": "application/json" },
        body: JSON.stringify({
          commit_title: pr.title || `Merge PR #${prNum}`,
          merge_method: "squash",
        }),
      });

      if (mergeResp.ok) {
        return `PR #${prNum} has been merged successfully into ${repo}, sir.`;
      } else {
        return githubErrorMessage(mergeResp.status, `merge PR #${prNum} in ${repo}`);
      }
    } catch (err) {
      return `Error merging PR: ${(err as Error).message}`;
    }
  }

  // ─── APPROVE PR ──────────────────────────────────────────────────
  // "approve PR #123", "approve pull request 123"
  if (/\bapprove\b/.test(t) && /\bpr\s*#?\s*\d+\b/.test(t)) {
    if (!repo) return "Which repository is this PR in? Say 'approve PR 123 in owner/repo'.";
    const prNum = (t.match(/\bpr\s*#?\s*(\d+)\b/) || [])[1];
    if (!prNum) return "Which PR number should I approve?";

    try {
      const resp = await fetch(`https://api.github.com/repos/${repo}/pulls/${prNum}/reviews`, {
        method: "POST",
        headers: { ...headers, "Content-Type": "application/json" },
        body: JSON.stringify({ event: "APPROVE" }),
      });

      if (resp.ok) {
        return `PR #${prNum} in ${repo} has been approved, sir.`;
      } else {
        return githubErrorMessage(resp.status, `approve PR #${prNum} in ${repo}`);
      }
    } catch (err) {
      return `Error approving PR: ${(err as Error).message}`;
    }
  }

  // ─── CLOSE PR ────────────────────────────────────────────────────
  // "close PR #123"
  if (/\bclose\b/.test(t) && /\bpr\s*#?\s*\d+\b/.test(t)) {
    if (!repo) return "Which repository is this PR in? Say 'close PR 123 in owner/repo'.";
    const prNum = (t.match(/\bpr\s*#?\s*(\d+)\b/) || [])[1];
    if (!prNum) return "Which PR number should I close?";

    const isConfirmed = /\b(yes|confirm|do it|go ahead|proceed)\b/.test(t);
    if (!isConfirmed) {
      return `Are you sure you want to close PR #${prNum} in ${repo}? Say "yes close PR ${prNum} in ${repo}" to confirm.`;
    }

    try {
      const resp = await fetch(`https://api.github.com/repos/${repo}/pulls/${prNum}`, {
        method: "PATCH",
        headers: { ...headers, "Content-Type": "application/json" },
        body: JSON.stringify({ state: "closed" }),
      });

      if (resp.ok) {
        return `PR #${prNum} in ${repo} has been closed, sir.`;
      } else {
        return githubErrorMessage(resp.status, `close PR #${prNum} in ${repo}`);
      }
    } catch (err) {
      return `Error closing PR: ${(err as Error).message}`;
    }
  }

  // ─── COMMENT ON PR ───────────────────────────────────────────────
  // "comment on PR #123 saying <text>"
  if (/\bcomment\b/.test(t) && /\bpr\s*#?\s*\d+\b/.test(t)) {
    if (!repo) return "Which repository is this PR in? Say 'comment on PR 123 in owner/repo saying <text>'.";
    const prNum = (t.match(/\bpr\s*#?\s*(\d+)\b/) || [])[1];
    if (!prNum) return "Which PR number should I comment on?";

    // Extract comment text after "saying"
    const sayingMatch = transcript.match(/saying\s+(.+)/i);
    const commentText = sayingMatch ? sayingMatch[1].trim() : "";
    if (!commentText) return "What should I say in the comment? Say 'comment on PR 123 saying <text>'.";

    try {
      const resp = await fetch(`https://api.github.com/repos/${repo}/issues/${prNum}/comments`, {
        method: "POST",
        headers: { ...headers, "Content-Type": "application/json" },
        body: JSON.stringify({ body: commentText }),
      });

      if (resp.ok) {
        return `Comment posted on PR #${prNum} in ${repo}, sir.`;
      } else {
        return githubErrorMessage(resp.status, `comment on PR #${prNum} in ${repo}`);
      }
    } catch (err) {
      return `Error commenting on PR: ${(err as Error).message}`;
    }
  }

  // ─── CLOSE ISSUE ─────────────────────────────────────────────────
  // "close issue #45"
  if (/\bclose\b/.test(t) && /\bissue\s*#?\s*\d+\b/.test(t)) {
    if (!repo) return "Which repository is this issue in? Say 'close issue 45 in owner/repo'.";
    const issueNum = (t.match(/\bissue\s*#?\s*(\d+)\b/) || [])[1];
    if (!issueNum) return "Which issue number should I close?";

    const isConfirmed = /\b(yes|confirm|do it|go ahead|proceed)\b/.test(t);
    if (!isConfirmed) {
      return `Are you sure you want to close issue #${issueNum} in ${repo}? Say "yes close issue ${issueNum} in ${repo}" to confirm.`;
    }

    try {
      const resp = await fetch(`https://api.github.com/repos/${repo}/issues/${issueNum}`, {
        method: "PATCH",
        headers: { ...headers, "Content-Type": "application/json" },
        body: JSON.stringify({ state: "closed" }),
      });

      if (resp.ok) {
        return `Issue #${issueNum} in ${repo} has been closed, sir.`;
      } else {
        return githubErrorMessage(resp.status, `close issue #${issueNum} in ${repo}`);
      }
    } catch (err) {
      return `Error closing issue: ${(err as Error).message}`;
    }
  }

  // ─── CREATE ISSUE ────────────────────────────────────────────────
  // "create issue titled <title> in owner/repo"
  if (/\b(create|open)\b/.test(t) && /\bissue\b/.test(t)) {
    if (!repo) return "Which repository should I create the issue in? Say 'create issue titled <title> in owner/repo'.";
    const titleMatch = transcript.match(/titled\s+(.+?)(?:\s+in\s+|$)/i);
    const title = titleMatch ? titleMatch[1].trim() : "";
    if (!title) return "What should the issue title be? Say 'create issue titled <title> in owner/repo'.";

    try {
      const resp = await fetch(`https://api.github.com/repos/${repo}/issues`, {
        method: "POST",
        headers: { ...headers, "Content-Type": "application/json" },
        body: JSON.stringify({ title }),
      });

      if (resp.ok) {
        const issue = await resp.json() as any;
        return `Issue #${issue.number} created in ${repo} with title "${title}", sir.`;
      } else {
        return githubErrorMessage(resp.status, `create issue in ${repo}`);
      }
    } catch (err) {
      return `Error creating issue: ${(err as Error).message}`;
    }
  }

  // ─── CREATE PR ───────────────────────────────────────────────────
  // "create a PR titled <title> from <branch> in owner/repo"
  if (/\b(create|open)\b/.test(t) && /\bpr|pull\s*request\b/.test(t)) {
    if (!repo) return "Which repository should I create the PR in? Say 'create PR titled <title> from <branch> in owner/repo'.";
    const titleMatch = transcript.match(/titled\s+(.+?)(?:\s+from\s+|\s+in\s+)/i);
    const title = titleMatch ? titleMatch[1].trim() : "";
    if (!title) return "What should the PR title be? Say 'create PR titled <title> from <branch> in owner/repo'.";

    // Extract source branch
    const branchMatch = transcript.match(/from\s+([a-zA-Z0-9_\-\/]+)/i);
    const head = branchMatch ? branchMatch[1] : "";
    if (!head) return "Which branch should I create the PR from? Say 'create PR titled <title> from <branch> in owner/repo'.";

    // Get default branch for base
    try {
      const metaResp = await fetch(`https://api.github.com/repos/${repo}`, { headers });
      if (!metaResp.ok) return githubErrorMessage(metaResp.status, `create PR in ${repo}`);
      const meta = await metaResp.json() as any;
      const base = meta.default_branch || "main";

      const prResp = await fetch(`https://api.github.com/repos/${repo}/pulls`, {
        method: "POST",
        headers: { ...headers, "Content-Type": "application/json" },
        body: JSON.stringify({ title, head, base }),
      });

      if (prResp.ok) {
        const pr = await prResp.json() as any;
        return `PR #${pr.number} created in ${repo} from ${head} to ${base}, sir. Title: "${title}".`;
      } else {
        return githubErrorMessage(prResp.status, `create PR in ${repo}`);
      }
    } catch (err) {
      return `Error creating PR: ${(err as Error).message}`;
    }
  }

  return "I can help you with GitHub write operations. Try saying 'merge PR 123 in owner/repo', 'approve PR 123', 'close issue 45', 'create issue titled <title> in owner/repo', or 'create PR titled <title> from <branch> in owner/repo'.";
}

// ---- GitHub deep analysis handler (GLM-5.2) ----

/**
 * Parse a PR number and repo from the transcript.
 * Patterns:
 *   "analyse PR 24 in zync"        → pr=24, repo=zync
 *   "analyse the PR of zync"       → latest PR, repo=zync
 *   "review PR 76 in owner/repo"   → pr=76, repo=owner/repo
 *   "analyse the pull request"     → latest PR, default repo
 */
function parsePRRequest(transcript: string): { prNumber: number | null; repoName: string | null } {
  const tLower = transcript.toLowerCase();

  // "PR 24", "PR #24", "pull request 24", "PR number 24" (STT variation)
  const prNumMatch = tLower.match(/(?:pr|pull\s*request)\s*(?:number|#\s*)?\s*#?\s*(\d+)/);
  const prNumber = prNumMatch ? parseInt(prNumMatch[1], 10) : null;

  // "in zync", "of zync", "on NEXUS agent", "from owner/repo",
  // "in ledger ai", "in ledger-ai" — support multi-word repo names
  // Exclude "of PR" and "of pull" — those are not repo names
  // Also exclude common English words that follow "of/in/from/on"
  const repoMatch = tLower.match(/(?:in|of|from|on)\s+(?!pr\b|pull\b|the\b|this\b|that\b|a\b|an\b)([\w\-./]+(?:\s+[\w\-./]+)?)/);
  if (!repoMatch || !repoMatch[1]) {
    return { prNumber, repoName: null };
  }
  // Extract from original transcript to preserve case
  const repoLower = repoMatch[1].trim();
  const idx = tLower.indexOf(repoLower);
  const repoName = idx >= 0 ? transcript.substr(idx, repoLower.length) : repoLower;

  return { prNumber, repoName };
}

/**
 * Parse a branch name and repo from the transcript.
 * Patterns:
 *   "analyse branch sidebar-markdown-rich-rendering in zync"
 *   "analyse the branch feature-auth in servx"
 *   "analyse branch main in ledger-ai"
 */
function parseBranchRequest(transcript: string): { branchName: string | null; repoName: string | null } {
  const tLower = transcript.toLowerCase();

  // Extract branch name: "branch <name>" or "the branch <name>"
  // Branch names can contain hyphens, underscores, slashes, and dots
  const branchMatch = tLower.match(/(?:branch|ranch|bench)\s+(?:the\s+)?([\w\-./]+)/);
  const branchName = branchMatch ? branchMatch[1] : null;

  // Extract repo name (same logic as parsePRRequest)
  const repoMatch = tLower.match(/(?:in|of|from|on)\s+(?!pr\b|pull\b|the\b|this\b|that\b|a\b|an\b|branch\b)([\w\-./]+(?:\s+[\w\-./]+)?)/);
  if (!repoMatch || !repoMatch[1]) {
    return { branchName, repoName: null };
  }
  const repoLower = repoMatch[1].trim();
  const idx = tLower.indexOf(repoLower);
  const repoName = idx >= 0 ? transcript.substr(idx, repoLower.length) : repoLower;

  return { branchName, repoName };
}

/**
 * Resolve a repo name to a full owner/repo string by searching the user's repos.
 * If the name already contains "/", use it as-is.
 * Otherwise, search the user's repos for a case-insensitive match.
 */
async function resolveRepo(token: string, repoName: string | null): Promise<{ full_name: string | null; availableRepos: string[] }> {
  if (!repoName) {
    // No repo specified — return null (caller will handle)
    return { full_name: null, availableRepos: [] };
  }
  if (repoName.includes("/")) {
    return { full_name: repoName, availableRepos: [] };
  }

  // Search user's repos for a case-insensitive match
  try {
    const resp = await fetch(
      `https://api.github.com/user/repos?per_page=100&sort=updated`,
      { headers: { "Authorization": `Bearer ${token}`, "Accept": "application/vnd.github+json", "User-Agent": "NEXUS-Worker" } },
    );
    if (!resp.ok) return { full_name: null, availableRepos: [] };
    const repos = await resp.json() as Array<Record<string, unknown>>;
    // Collect available repo names for the error message
    const availableRepos = repos
      .map(r => (r["name"] as string) || "")
      .filter(n => n.length > 0)
      .slice(0, 15);  // top 15 most recently updated

    // Normalize: "ledger ai" → "ledger-ai" for matching (GitHub repos use hyphens)
    const target = repoName.toLowerCase().replace(/\s+/g, "-");

    // 1. Try exact match first
    const exact = repos.find(r => (r["name"] as string)?.toLowerCase() === target);
    if (exact) return { full_name: exact["full_name"] as string, availableRepos };

    // 2. Try partial match (target is a substring of repo name, or vice versa)
    const partial = repos.find(r => {
      const name = (r["name"] as string)?.toLowerCase();
      return name && (name.includes(target) || target.includes(name));
    });
    if (partial) return { full_name: partial["full_name"] as string, availableRepos };

    // 3. FUZZY MATCH: STT often mishears repo names (e.g. "servx" → "service",
    //    "weeks", "serve x"). Use Levenshtein distance + prefix matching to
    //    find the closest repo name. This handles phonetic mishearings.
    const repoNames = repos
      .map(r => (r["name"] as string) || "")
      .filter(n => n.length > 0);

    let bestMatch: string | null = null;
    let bestScore = Infinity;

    for (const repoName_ of repoNames) {
      const candidate = repoName_.toLowerCase();
      // Skip repos that are too different in length (avoid matching "a" to "servx")
      if (Math.abs(candidate.length - target.length) > Math.max(3, target.length)) continue;

      // Levenshtein distance
      const dist = levenshtein(target, candidate);
      // Normalised score: distance / max_length (0 = perfect match, 1 = totally different)
      const score = dist / Math.max(target.length, candidate.length);

      // Also check prefix match (first 3 chars) — "ser" matches "servx" and "service"
      const prefixLen = Math.min(3, Math.min(target.length, candidate.length));
      const prefixMatch = target.substring(0, prefixLen) === candidate.substring(0, prefixLen);

      // If prefix matches, reduce the effective score (bonus for phonetic similarity)
      const adjustedScore = prefixMatch ? score * 0.5 : score;

      // Accept if normalised distance < 0.6 (i.e. >40% of chars match)
      if (adjustedScore < 0.6 && adjustedScore < bestScore) {
        bestScore = adjustedScore;
        bestMatch = repoName_;
      }
    }

    if (bestMatch) {
      const matched = repos.find(r => (r["name"] as string) === bestMatch);
      if (matched) return { full_name: matched["full_name"] as string, availableRepos };
    }

    // 4. PHONETIC MATCH: STT can mishear names so badly that Levenshtein fails
    //    entirely (e.g. "ledgerAI" → "luxury" — no shared characters). Use a
    //    simple phonetic encoding (first letter + consonant skeleton) to match
    //    names that sound similar even if they're spelled completely differently.
    const targetPhonetic = phoneticSkeleton(target);
    let phoneticMatch: string | null = null;
    let phoneticBestScore = Infinity;

    for (const repoName_ of repoNames) {
      const candidate = repoName_.toLowerCase().replace(/[^a-z]/g, "");
      const candidatePhonetic = phoneticSkeleton(candidate);
      // Compare phonetic skeletons using Levenshtein
      const pDist = levenshtein(targetPhonetic, candidatePhonetic);
      const pScore = pDist / Math.max(targetPhonetic.length, candidatePhonetic.length);
      // Accept if phonetic distance < 0.5 (sounds similar)
      if (pScore < 0.5 && pScore < phoneticBestScore) {
        phoneticBestScore = pScore;
        phoneticMatch = repoName_;
      }
    }

    if (phoneticMatch) {
      const matched = repos.find(r => (r["name"] as string) === phoneticMatch);
      if (matched) return { full_name: matched["full_name"] as string, availableRepos };
    }

    return { full_name: null, availableRepos };
  } catch {
    return { full_name: null, availableRepos: [] };
  }
}

/**
 * Simple phonetic skeleton: extract the first letter + consonant sounds.
 * This is a simplified Soundex-like encoding that groups similar-sounding
 * consonants together. Used as a last-resort fallback when Levenshtein
 * fuzzy matching fails (e.g. STT mishears "ledgerAI" as "luxury").
 *
 * Groups: b/p/f/v → 1, c/k/g/j/q/s/x/z → 2, d/t → 3, l → 4,
 *         m/n → 5, r → 6, h/w/y → dropped, vowels → dropped
 */
function phoneticSkeleton(s: string): string {
  if (!s) return "";
  const map: Record<string, string> = {
    b: "1", p: "1", f: "1", v: "1",
    c: "2", k: "2", g: "2", j: "2", q: "2", s: "2", x: "2", z: "2",
    d: "3", t: "3",
    l: "4",
    m: "5", n: "5",
    r: "6",
  };
  const first = s[0];
  let result = first;
  let prevCode = map[first] || "";
  for (let i = 1; i < s.length; i++) {
    const c = s[i];
    const code = map[c] || "";
    if (code && code !== prevCode) {
      result += code;
    }
    if (code) prevCode = code;
  }
  return result;
}

/**
 * Levenshtein edit distance between two strings.
 * Used for fuzzy repo name matching when STT mishears the name.
 */
function levenshtein(a: string, b: string): number {
  const m = a.length;
  const n = b.length;
  if (m === 0) return n;
  if (n === 0) return m;

  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));
  for (let i = 0; i <= m; i++) dp[i][0] = i;
  for (let j = 0; j <= n; j++) dp[0][j] = j;

  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      dp[i][j] = Math.min(
        dp[i - 1][j] + 1,
        dp[i][j - 1] + 1,
        dp[i - 1][j - 1] + cost,
      );
    }
  }
  return dp[m][n];
}

/**
 * Fetch full PR context via GitHub REST API (no cloning needed).
 * Returns: metadata, files with diffs, commits, and review comments.
 */
/** Result of fetchPRContext — includes both the text context for the LLM
 * and the raw PR stats for deterministic section generation. */
interface PRContextResult {
  context: string;
  stats: {
    insertions: number;
    deletions: number;
    filesChanged: number;
    commits: number;
    mergeableState: string;
    hasMergeConflicts: boolean;
  } | null;
}

async function fetchPRContext(
  token: string,
  repo: string,
  prNumber: number,
): Promise<PRContextResult> {
  const headers: Record<string, string> = {
    "Authorization": `Bearer ${token}`,
    "Accept": "application/vnd.github+json",
    "X-GitHub-Api-Version": "2022-11-28",
    "User-Agent": "NEXUS-Worker",
  };

  // 1. PR metadata
  const prResp = await fetch(`https://api.github.com/repos/${repo}/pulls/${prNumber}`, { headers });
  if (!prResp.ok) {
    if (prResp.status === 404) return { context: `__ERROR__: PR #${prNumber} not found in ${repo}.`, stats: null };
    if (prResp.status === 401) return { context: `__ERROR__: ${githubErrorMessage(401, `analyse PR #${prNumber}`)}`, stats: null };
    return { context: `__ERROR__: GitHub API returned ${prResp.status} for PR #${prNumber}.`, stats: null };
  }
  const pr = await prResp.json() as Record<string, unknown>;

  // Extract deterministic stats for the structured output sections
  const prStats = {
    insertions: (pr["additions"] as number) || 0,
    deletions: (pr["deletions"] as number) || 0,
    filesChanged: (pr["changed_files"] as number) || 0,
    commits: (pr["commits"] as number) || 0,
    mergeableState: (pr["mergeable_state"] as string) || "unknown",
    hasMergeConflicts: (pr["mergeable_state"] as string) === "dirty" || pr["mergeable"] === false,
  };

  // 2. Files with diffs (parallel with commits + comments)
  const [filesResp, commitsResp, commentsResp, reviewsResp] = await Promise.all([
    fetch(`https://api.github.com/repos/${repo}/pulls/${prNumber}/files?per_page=100`, { headers }),
    fetch(`https://api.github.com/repos/${repo}/pulls/${prNumber}/commits?per_page=100`, { headers }),
    fetch(`https://api.github.com/repos/${repo}/pulls/${prNumber}/comments?per_page=50`, { headers }),
    fetch(`https://api.github.com/repos/${repo}/pulls/${prNumber}/reviews?per_page=50`, { headers }),
  ]);

  const files = filesResp.ok ? await filesResp.json() as Array<Record<string, unknown>> : [];
  const commits = commitsResp.ok ? await commitsResp.json() as Array<Record<string, unknown>> : [];
  const comments = commentsResp.ok ? await commentsResp.json() as Array<Record<string, unknown>> : [];
  const reviews = reviewsResp.ok ? await reviewsResp.json() as Array<Record<string, unknown>> : [];

  // 3. Assemble context (truncate to stay under 250K tokens)
  const meta = `PR #${pr["number"]}: ${pr["title"]}
Repository: ${repo}
State: ${pr["state"]} | Draft: ${pr["draft"] ? "yes" : "no"} | Mergeable: ${pr["mergeable_state"] || "unknown"}
Author: ${(pr["user"] as Record<string, string>)?.login || "unknown"}
Created: ${pr["created_at"]} | Updated: ${pr["updated_at"]}
Branch: ${pr["head"] ? (pr["head"] as Record<string, string>).ref : "unknown"} → ${pr["base"] ? (pr["base"] as Record<string, string>).ref : "unknown"}
Changes: +${pr["additions"]} -${pr["deletions"]} across ${pr["changed_files"]} files
Commits: ${pr["commits"]}

Description:
${(pr["body"] as string || "(no description provided)").slice(0, 2000)}`;

  // Files with diffs — keep patches but truncate very long ones
  const MAX_PATCH_PER_FILE = 3000;
  const MAX_TOTAL_PATCH = 200000;  // ~50K tokens
  let totalPatchLen = 0;
  const fileSections: string[] = [];

  for (const file of files) {
    const filename = file["filename"] as string;
    const status = file["status"] as string;
    const additions = file["additions"] as number;
    const deletions = file["deletions"] as number;
    let patch = (file["patch"] as string) || "(binary file or no patch available)";

    if (patch.length > MAX_PATCH_PER_FILE) {
      patch = patch.slice(0, MAX_PATCH_PER_FILE) + "\n... (truncated)";
    }

    if (totalPatchLen + patch.length > MAX_TOTAL_PATCH) {
      fileSections.push(`--- ${filename} (${status}, +${additions} -${deletions})\n(diff truncated — too many changes)`);
      continue;
    }
    totalPatchLen += patch.length;

    fileSections.push(`--- ${filename} (${status}, +${additions} -${deletions})
${patch}`);
  }

  // Commits
  const commitList = commits.slice(0, 30).map((c) => {
    const msg = (c["commit"] as Record<string, { message?: string }>)?.commit?.message || "";
    const author = (c["commit"] as Record<string, { author?: { name?: string } }>)?.commit?.author?.name || "unknown";
    return `  ${c["sha"]?.toString().slice(0, 7)} ${msg.split("\n")[0]} (${author})`;
  }).join("\n");

  // Review comments (inline code comments)
  const commentList = comments.slice(0, 30).map((c) => {
    const user = (c["user"] as Record<string, string>)?.login || "unknown";
    const body = (c["body"] as string || "").slice(0, 500);
    const path = c["path"] as string;
    const line = c["line"] || c["original_line"] || "?";
    return `  ${user} on ${path}:${line}: ${body}`;
  }).join("\n");

  // Reviews
  const reviewList = reviews.slice(0, 20).map((r) => {
    const user = (r["user"] as Record<string, string>)?.login || "unknown";
    const state = r["state"] as string;
    const body = (r["body"] as string || "").slice(0, 500);
    return `  ${user}: ${state}${body ? ` — ${body}` : ""}`;
  }).join("\n");

  const context = `${meta}

=== FILES CHANGED (${files.length}) ===
${fileSections.join("\n\n")}

=== COMMITS (${commits.length}) ===
${commitList || "(none)"}

=== REVIEW COMMENTS (${comments.length}) ===
${commentList || "(none)"}

=== REVIEWS (${reviews.length}) ===
${reviewList || "(none)"}`;

  return { context, stats: prStats };
}

/**
 * Deep PR analysis using GLM-5.2.
 * Fetches PR data via GitHub API, sends to GLM-5.2 for analysis.
 */
/**
 * Detect if this is a re-evaluation request.
 * Triggers on phrases like "re-analyse", "deeper review", "re-evaluate", "again".
 * Also checks if the same PR was analysed recently (within 5 minutes).
 */
function isReEvaluationRequest(transcript: string, userId: string, repo: string, prNumber: number): boolean {
  const t = transcript.toLowerCase();
  // Explicit re-evaluation keywords
  if (/\b(re-?analy[sz]e|re-?evaluat|deeper\s+review|again|re-?review|more\s+thorough|take\s+another\s+look)\b/.test(t)) {
    return true;
  }
  // Same PR analysed recently → assume re-evaluation
  const key = `${userId}:${repo}:${prNumber}`;
  const lastTime = recentAnalyses.get(key);
  if (lastTime && (Date.now() - lastTime) < RE_EVALUATION_WINDOW_MS) {
    return true;
  }
  return false;
}

async function handleGitHubAnalyse(req: NexusRequest, env: Env, token: string): Promise<string> {
  const tLower = req.task.request.toLowerCase();

  // Branch analysis: "analyse branch X in repo" → delegate to branch handler
  if (/\bbranch\b/.test(tLower) && !/\bpr\b|\bpull\s*request\b/.test(tLower)) {
    return handleBranchAnalyse(req, env, token);
  }

  const { prNumber, repoName } = parsePRRequest(req.task.request);
  const userId = req.requester.id;

  try {
    // Resolve the repo name to a full owner/repo
    const repoResult = await resolveRepo(token, repoName);
    const repo = repoResult.full_name;
    if (!repo) {
      const repoList = repoResult.availableRepos.length > 0
        ? `\n\nYour available repositories: ${repoResult.availableRepos.join(", ")}`
        : "";
      return `I couldn't find a repository matching "${repoName}" in your GitHub account. Try specifying the full name, like "analyse PR 24 in owner/repo".${repoList}`;
    }

    let actualPrNumber = prNumber;

    // If no PR number specified, get the most recent PR (open or closed)
    if (!actualPrNumber) {
      const headers: Record<string, string> = {
        "Authorization": `Bearer ${token}`,
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "NEXUS-Worker",
      };
      const resp = await fetch(`https://api.github.com/repos/${repo}/pulls?state=all&per_page=1&sort=created&direction=desc`, { headers });
      if (!resp.ok) {
        if (resp.status === 401) return githubErrorMessage(401, `find PRs in ${repo}`);
        return `I couldn't find any pull requests in ${repo}. Error: ${resp.status}. Try saying "analyse PR 24 in ${repo}".`;
      }
      const prs = await resp.json() as Array<Record<string, unknown>>;
      if (!prs || prs.length === 0) {
        return `There are no pull requests in ${repo}. Try specifying a PR number, like "analyse PR 24".`;
      }
      actualPrNumber = prs[0]["number"] as number;
    }

    // Fetch full PR context
    const prContextResult = await fetchPRContext(token, repo, actualPrNumber);

    if (prContextResult.context.startsWith("__ERROR__:")) {
      return prContextResult.context.replace("__ERROR__:", "");
    }
    const context = prContextResult.context;
    const prStats = prContextResult.stats;

    // Determine which model to use:
    // 1. Re-evaluation request → deep model (GLM-5.3-Flash, 1M context)
    // 2. Context exceeds 131K tokens → deep model (GLM-5.3-Flash, 1M context)
    // 3. Default → primary model (GLM-4.7-Flash, 131K context, 10x cheaper)
    const isReEval = isReEvaluationRequest(req.task.request, userId, repo, actualPrNumber);
    const contextTooLarge = context.length > FLASH_CONTEXT_LIMIT_CHARS;
    const useDeepModel = isReEval || contextTooLarge;

    // ── Cache check: skip LLM if we already analysed this exact PR context ──
    // Re-evaluation requests always bypass cache (user wants a fresh review).
    const ctxHash = contentHash(context);
    const cacheKey = prAnalysisKey(userId, repo, actualPrNumber, ctxHash);
    if (!isReEval) {
      const cached = await cacheGet<string>(env, cacheKey, 7200); // 2h TTL
      if (cached) {
        console.log(`[cache] PR analysis HIT: ${repo}#${actualPrNumber} (saved LLM call)`);
        const repoShort = repo.includes("/") ? repo.split("/")[1] : repo;
        return `PR #${actualPrNumber} in ${repoShort}\n\n${cached}`;
      }
    }
    const model = useDeepModel ? DEEP_ANALYSIS_MODEL : ANALYSIS_MODEL;

    // Record this analysis for re-evaluation detection
    const analysisKey = `${userId}:${repo}:${actualPrNumber}`;
    recentAnalyses.set(analysisKey, Date.now());

    // Clean up old entries (keep map small)
    if (recentAnalyses.size > 100) {
      const now = Date.now();
      for (const [k, t] of recentAnalyses) {
        if (now - t > RE_EVALUATION_WINDOW_MS) recentAnalyses.delete(k);
      }
    }

    const modelLabel = useDeepModel
      ? (isReEval ? "DEEP REVIEW (re-evaluation)" : "DEEP REVIEW (large PR)")
      : "CODE REVIEW";

    // Build analysis prompt
    const analysisPrompt = `You are a senior software engineer performing a thorough code review. Analyse this pull request and provide a structured review.

Format your response EXACTLY as follows (use Markdown):

## How It Helps the Existing Codebase
Explain how this PR impacts and benefits the existing codebase/repo. Reference specific files, modules, or architecture patterns it touches. (3-4 sentences)

## Bugs Found

| Severity | Description | Source File |
|----------|-------------|-------------|
| Critical | <bug description> | \`path/to/file.ext\` |
| High | <bug description> | \`path/to/file.ext\` |
| Medium | <bug description> | \`path/to/file.ext\` |
| Low | <bug description> | \`path/to/file.ext\` |

Rules for the bug table:
- Severity column: use "Critical", "High", "Medium", or "Low" — sorted from most to least severe
- Description column: concise explanation of the bug (1-2 sentences)
- Source File column: the exact file path where the bug is found, in backticks
- Only include bugs you are confident about — do not invent issues
- If no bugs are found, write "No bugs found." instead of the table

## Verdict
Is this safe to merge? (Approve / Request Changes / Block) — one sentence explanation.

Be specific — reference actual file names from the PR diff.
If the PR is small or straightforward, keep the review concise.

${useDeepModel ? "Note: This is a DEEP review — be extra thorough. Check edge cases, error handling, test coverage gaps, and security implications that might be missed in a quick review." : ""}

=== PULL REQUEST CONTEXT ===
${context}

=== YOUR ANALYSIS ===`;

    // GLM-4.7-Flash is a reasoning model — needs more tokens for reasoning + answer
    // GLM-5.3-Flash is more efficient but still needs adequate output space
    const maxTokens = useDeepModel ? 2500 : 3000;

    const response = await env.AI.run(model as any, {
      messages: [
        { role: "system", content: "You are a senior software engineer with expertise in code review, security, and software architecture. You provide thorough, actionable code reviews." },
        { role: "user", content: analysisPrompt },
      ],
      max_tokens: maxTokens,
    });

    const analysis = extractText(response);
    if (!analysis) {
      return `I fetched PR #${actualPrNumber} in ${repo} but couldn't generate an analysis. The PR has ${(context.match(/---/g) || []).length} changed files. Try asking a more specific question about it.`;
    }

    // Prefix with the resolved repo name so the user sees what NEXUS understood
    // (e.g. "PR #63 in ledger-ai" instead of the misheard "lageria")
    const repoShort = repo.includes("/") ? repo.split("/")[1] : repo;
    const understoodPrefix = `PR #${actualPrNumber} in ${repoShort}\n\n`;

    // ── Deterministic sections (from GitHub API, not LLM) ──
    // These are appended after the LLM output so they are always accurate.
    let deterministicSection = "";
    if (prStats) {
      const conflictStatus = prStats.hasMergeConflicts
        ? "Yes — merge conflicts detected. Resolve before merging."
        : prStats.mergeableState === "unknown"
          ? "Unknown — GitHub is still computing mergeability. Check again shortly."
          : "No — this PR can be merged cleanly.";
      deterministicSection = `

## Stats

| Metric | Value |
|--------|-------|
| Insertions | +${prStats.insertions} |
| Deletions | -${prStats.deletions} |
| Files changed | ${prStats.filesChanged} |
| Commits | ${prStats.commits} |

## Merge Conflicts

**${conflictStatus}**`;
    }

    // Prefix deep reviews so the user knows which model was used
    const finalText = useDeepModel
      ? `${understoodPrefix}[${modelLabel}] ${analysis}${deterministicSection}`
      : `${understoodPrefix}${analysis}${deterministicSection}`;

    // ── Cache the analysis result (2h TTL) ──
    // Store without the understoodPrefix so cached text is reusable.
    // The prefix is re-added on cache hit.
    if (!isReEval) {
      const cacheText = useDeepModel ? `[${modelLabel}] ${analysis}${deterministicSection}` : `${analysis}${deterministicSection}`;
      await cacheSet(env, cacheKey, cacheText, 7200);
      console.log(`[cache] PR analysis stored: ${repo}#${actualPrNumber} (TTL 2h)`);
    }

    return finalText;
  } catch (err) {
    return `I had trouble analysing the PR. Error: ${(err as Error).message}`;
  }
}

/**
 * Deep branch analysis using GLM.
 * Fetches the branch's commits and diff against the default branch (main/master),
 * sends to GLM for analysis.
 */
async function handleBranchAnalyse(req: NexusRequest, env: Env, token: string): Promise<string> {
  const { branchName, repoName } = parseBranchRequest(req.task.request);
  const userId = req.requester.id;

  if (!branchName) {
    return `I couldn't identify which branch you want to analyse. Try saying "analyse branch feature-name in repo-name".`;
  }

  try {
    // Resolve the repo name
    const repoResult = await resolveRepo(token, repoName);
    const repo = repoResult.full_name;
    if (!repo) {
      const repoList = repoResult.availableRepos.length > 0
        ? `\n\nYour available repositories: ${repoResult.availableRepos.join(", ")}`
        : "";
      return `I couldn't find a repository matching "${repoName}" in your GitHub account. Try specifying the full name, like "analyse branch ${branchName} in owner/repo".${repoList}`;
    }

    const headers: Record<string, string> = {
      "Authorization": `Bearer ${token}`,
      "Accept": "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28",
      "User-Agent": "NEXUS-Worker",
    };

    // 1. Verify the branch exists
    const branchResp = await fetch(`https://api.github.com/repos/${repo}/branches/${encodeURIComponent(branchName)}`, { headers });
    if (!branchResp.ok) {
      return `I couldn't find a branch named "${branchName}" in ${repo}. Error: ${branchResp.status}. Check the branch name and try again.`;
    }

    // 2. Get the default branch (main or master) for comparison
    const repoInfoResp = await fetch(`https://api.github.com/repos/${repo}`, { headers });
    const repoInfo = await repoInfoResp.json() as Record<string, unknown>;
    const defaultBranch = (repoInfo["default_branch"] as string) || "main";

    // 3. Compare the branch against the default branch
    const compareResp = await fetch(
      `https://api.github.com/repos/${repo}/compare/${defaultBranch}...${encodeURIComponent(branchName)}`,
      { headers },
    );
    if (!compareResp.ok) {
      return `I couldn't compare branch "${branchName}" against ${defaultBranch} in ${repo}. Error: ${compareResp.status}.`;
    }
    const compareData = await compareResp.json() as Record<string, unknown>;

    const aheadBy = compareData["ahead_by"] as number;
    const behindBy = compareData["behind_by"] as number;
    const totalCommits = compareData["total_commits"] as number;
    const files = compareData["files"] as Array<Record<string, unknown>>;
    const commits = compareData["commits"] as Array<Record<string, unknown>>;

    if (!files || files.length === 0) {
      return `Branch "${branchName}" in ${repo} has no changes compared to ${defaultBranch}. It's already up to date.`;
    }

    // 4. Build context for GLM analysis
    const fileList = files.slice(0, 30).map((f) => {
      const status = f["status"] as string;
      const filename = f["filename"] as string;
      const additions = f["additions"] as number;
      const deletions = f["deletions"] as number;
      const patch = (f["patch"] as string || "").slice(0, 500);
      return `--- ${status}: ${filename} (+${additions} -${deletions}) ---\n${patch}`;
    }).join("\n\n");

    const commitList = commits.slice(0, 20).map((c) => {
      const msg = (c["commit"] as Record<string, unknown>)?.["message"] as string || "";
      const author = ((c["commit"] as Record<string, unknown>)?.["author"] as Record<string, unknown>)?.["name"] as string || "unknown";
      return `- ${msg.split("\n")[0]} (${author})`;
    }).join("\n");

    const context = `=== BRANCH ANALYSIS ===
Repository: ${repo}
Branch: ${branchName}
Base: ${defaultBranch}
Ahead by: ${aheadBy} commits, Behind by: ${behindBy} commits
Total commits: ${totalCommits}
Changed files: ${files.length}

=== COMMITS (${commits.length}) ===
${commitList || "(none)"}

=== FILE CHANGES (top ${Math.min(files.length, 30)} of ${files.length}) ===
${fileList}`;

    // 5. Determine which model to use
    const contextTooLarge = context.length > FLASH_CONTEXT_LIMIT_CHARS;
    const model = contextTooLarge ? DEEP_ANALYSIS_MODEL : ANALYSIS_MODEL;
    const maxTokens = contextTooLarge ? 2500 : 3000;

    // 6. Send to GLM for analysis
    const analysisPrompt = `You are a senior software engineer performing a thorough branch analysis. Analyse this branch and provide a detailed review.

Cover these areas:
1. **Summary**: What does this branch do? (1-2 sentences)
2. **Risk Assessment**: Are there bugs, security issues, or breaking changes?
3. **Code Quality**: Is the code clean, well-structured, and maintainable?
4. **Suggestions**: What would you improve before merging?
5. **Verdict**: Is this safe to merge? (approve / request changes / block)

Be specific — reference file names when pointing out issues.
If the branch is small or straightforward, keep the review concise.

=== BRANCH CONTEXT ===
${context}

=== YOUR ANALYSIS ===`;

    const response = await env.AI.run(model as any, {
      messages: [
        { role: "system", content: "You are a senior software engineer with expertise in code review, security, and software architecture. You provide thorough, actionable code reviews." },
        { role: "user", content: analysisPrompt },
      ],
      max_tokens: maxTokens,
    });

    const analysis = extractText(response);
    if (!analysis) {
      return `I fetched branch "${branchName}" in ${repo} but couldn't generate an analysis. The branch has ${files.length} changed files. Try asking a more specific question about it.`;
    }

    const repoShort = repo.includes("/") ? repo.split("/")[1] : repo;
    const understoodPrefix = `Branch ${branchName} in ${repoShort}\n\n`;

    return `${understoodPrefix}${analysis}`;
  } catch (err) {
    return `I had trouble analysing the branch. Error: ${(err as Error).message}`;
  }
}

// ---- Gmail handler ----

async function handleGmail(req: NexusRequest, env: Env, token: string): Promise<string> {
  const transcript = req.task.request.toLowerCase();

  try {
    if (/\b(unread|inbox|recent|latest|new)\b/.test(transcript)) {
      const resp = await fetch(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages?q=is:unread&maxResults=5",
        { headers: { "Authorization": `Bearer ${token}` } }
      );
      if (!resp.ok) return `I couldn't access your Gmail. Error: ${resp.status}`;

      const data = await resp.json() as { messages?: Array<{ id: string }> };
      if (!data.messages || data.messages.length === 0) return "You have no unread emails. Your inbox is clean!";

      const emails = await Promise.all(
        data.messages.slice(0, 5).map(async (msg) => {
          const detail = await fetch(
            `https://gmail.googleapis.com/gmail/v1/users/me/messages/${msg.id}?format=metadata&metadataHeaders=From&metadataHeaders=Subject&metadataHeaders=Date`,
            { headers: { "Authorization": `Bearer ${token}` } }
          );
          if (!detail.ok) return null;
          const msgData = await detail.json() as { payload?: { headers?: Array<{ name: string; value: string }> } };
          const headers = msgData.payload?.headers || [];
          const from = headers.find(h => h.name === "From")?.value || "Unknown";
          const subject = headers.find(h => h.name === "Subject")?.value || "(no subject)";
          return `From: ${from}\nSubject: ${subject}`;
        })
      );

      const validEmails = emails.filter(Boolean);
      if (validEmails.length === 0) return "I found unread emails but couldn't read their details.";

      return await summarize(
        `The user asked about unread emails. Summarize these concisely (who they're from and the subject):\n\n${validEmails.join("\n\n")}`,
        env
      );
    }

    return await summarize(
      `The user asked: "${req.task.request}". This is a Gmail-related request. Suggest they try "check unread emails" or "what's in my inbox".`,
      env, false
    );
  } catch (err) {
    return `I had trouble reaching Gmail. Error: ${(err as Error).message}`;
  }
}

// ---- Calendar handler ----

async function handleCalendar(req: NexusRequest, env: Env, token: string): Promise<string> {
  try {
    const now = new Date();
    const timeMin = now.toISOString();
    const endOfDay = new Date(now);
    endOfDay.setHours(23, 59, 59, 999);
    const timeMax = endOfDay.toISOString();

    const resp = await fetch(
      `https://www.googleapis.com/calendar/v3/calendars/primary/events?timeMin=${timeMin}&timeMax=${timeMax}&singleEvents=true&orderBy=startTime&maxResults=10`,
      { headers: { "Authorization": `Bearer ${token}` } }
    );

    if (!resp.ok) return `I couldn't access your calendar. Error: ${resp.status}`;

    const data = await resp.json() as { items?: Array<Record<string, unknown>> };
    if (!data.items || data.items.length === 0) return "You have no events scheduled for the rest of today.";

    const events = data.items.map((evt, i) => {
      const start = (evt["start"] as Record<string, string>)?.dateTime || (evt["start"] as Record<string, string>)?.date || "Unknown time";
      const summary = evt["summary"] || "(no title)";
      return `${i + 1}. ${start}: ${summary}`;
    }).join("\n");

    return await summarize(`The user asked about their schedule. Summarize today's events concisely:\n\n${events}`, env);
  } catch (err) {
    return `I had trouble reaching Google Calendar. Error: ${(err as Error).message}`;
  }
}

// ---- Search / General handlers ----

async function handleSearch(req: NexusRequest, env: Env): Promise<string> {
  const query = req.task.request;
  const userId = req.requester.id;

  // Check cache first
  const cacheK = searchKey("en", query);
  const cached = await cacheGet<string>(env, cacheK, 86400); // 24h TTL
  if (cached) return cached;

  // Multi-source cascade retrieval (Wikipedia → DDG → knowledgelib →
  // SearchX → Tavily → Google → Serper, with Wolfram for math and
  // Semantic Scholar for academic)
  const retrievalResult = await retrieveCascade(query, env, 5);
  const deduped = dedupeSources(retrievalResult.results);

  if (deduped.length === 0) {
    // No sources found — fall back to LLM with honest framing
    const fallback = await summarize(
      `Answer this question concisely. If you're not confident in the answer, say so:\n\n${query}`,
      env
    );
    await incrementUsage(env, userId, { requests: 1, ai_neurons: 50, search_calls: 1 });
    return fallback;
  }

  // Build synthesis prompt with sources + prompt-injection guard
  const prompt = buildSearchSynthesisPrompt(query, { results: deduped, query, lang: "en" });

  // ── Section 1: Research (Groq synthesis) ──
  // Use Groq directly as the primary research synthesizer.
  // Fall back to the full cascade (Gemini → Groq → CF) if Groq fails.
  let synthesis = "";
  let synthProvider = "fallback";
  let synthModel = "raw";

  const groqResult = await callGroq(prompt, env,
    "You are NEXUS, a voice assistant. Answer the user's question directly and concisely using only the provided sources. Never show your reasoning, analysis steps, or thought process. Give only the final answer with citation numbers like [1], [2].",
    500,
  );
  if (groqResult) {
    synthesis = groqResult.text;
    synthProvider = groqResult.provider;
    synthModel = groqResult.model;
  } else {
    // Groq failed — fall back to the full cascade
    const llmResult = await synthesizeWithCascade(prompt, env);
    synthesis = llmResult?.text || "";
    synthProvider = llmResult?.provider || "fallback";
    synthModel = llmResult?.model || "raw";
  }
  console.log(`[search] synthesis via ${synthProvider} (${synthModel})`);

  if (!synthesis) {
    // All LLM providers failed — return the raw snippet from the best source
    synthesis = `${deduped[0].snippet}\n\nSource: ${deduped[0].url}`;
  }

  // Build source list (exclude Wikipedia — it gets its own section)
  const nonWikiSources = deduped.filter(r => r.source !== "wikipedia");
  const sourceList = nonWikiSources.map((r, i) => `[${i + 1}] ${r.title} — ${r.url}`).join("\n");

  // ── Section 2: Wikipedia (separate, verbatim extract) ──
  // Always fetch Wikipedia directly for this section — the cascade may
  // have skipped it (empty snippet) or returned it via SearchX/Tavily
  // with a different `source` field. A direct fetch guarantees we get
  // the full Wikipedia extract.
  const wikiResult = await searchWikipedia(query, "en");

  // ── Combine both sections ──
  let fullReply = `## Research\n\n${synthesis}`;

  if (sourceList) {
    fullReply += `\n\nSources:\n${sourceList}`;
  }

  if (wikiResult && wikiResult.snippet) {
    fullReply += `\n\n---\n\n## Wikipedia\n\n${wikiResult.snippet}\n\nRead more: ${wikiResult.url}`;
  }

  // Cache for 24 hours
  await cacheSet(env, cacheK, fullReply, 86400);

  // Track usage
  await incrementUsage(env, userId, { requests: 1, ai_neurons: 100, search_calls: 1 });

  return fullReply;
}

async function handleGeneral(req: NexusRequest, env: Env): Promise<string> {
  const prompt = `You are NEXUS, a helpful personal assistant. Answer the user's request concisely and naturally, as if speaking aloud:\n\n${req.task.request}`;
  const llmResult = await synthesizeWithCascade(
    prompt,
    env,
    "You are NEXUS, a helpful personal assistant. Answer concisely and naturally, as if speaking aloud. Never show reasoning steps.",
    500,
  );
  if (llmResult) return llmResult.text;
  // Fallback to Cloudflare summarize if cascade failed
  return await summarize(prompt, env);
}

// ---- Request types ----

interface NexusRequest {
  request_id: string;
  requester: { id: string; device_id: string };
  task: { type: string; request: string; dialog_context?: DialogContext };
}

/** Dialog context sent by the client on a follow-up turn. */
interface DialogContext {
  pending_intent: string;
  original_request: string;
  missing: string[];
}

/** Dialog state returned to the client when more info is needed. */
interface DialogState {
  expects_followup: boolean;
  pending_intent: string;
  context: DialogContext;
}

// ---- Main entry point ----

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const path = url.pathname;
    const method = request.method;

    const json = (data: unknown, status = 200) =>
      new Response(JSON.stringify(data), {
        status,
        headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" },
      });

    // ---- CORS preflight ----
    if (method === "OPTIONS") {
      return new Response(null, {
        headers: {
          "Access-Control-Allow-Origin": "*",
          "Access-Control-Allow-Methods": "GET, POST, DELETE, OPTIONS",
          "Access-Control-Allow-Headers": "Content-Type, Authorization",
        },
      });
    }

    // ---- Health ----
    if (path === "/health" && method === "GET") {
      return json({ ok: true, service: "NEXUS Worker", protocol: "text-only", serverless: true });
    }

    // ---- User registration ----
    if (path === "/api/register" && method === "POST") {
      return handleRegister(request, env, json);
    }

    // ---- OAuth: get auth URL ----
    if (path === "/oauth/auth-url" && method === "GET") {
      return handleAuthUrl(url, env, json);
    }

    // ---- OAuth: browser callback (for testing — exchanges code automatically) ----
    if (path === "/oauth/callback" && method === "GET") {
      return handleOAuthBrowserCallback(url, env, json);
    }

    // ---- OAuth: exchange code for tokens ----
    if (path === "/oauth/exchange" && method === "POST") {
      return handleOAuthExchange(request, env, json);
    }

    // ---- OAuth: status ----
    if (path === "/oauth/status" && method === "GET") {
      return handleOAuthStatus(url, env, json);
    }

    // ---- OAuth: get github token (for architect) ----
    if (path === "/oauth/github-token" && method === "GET") {
      const userId = url.searchParams.get("user_id") || "";
      if (!userId) return json({ error: "user_id required" }, 400);
      const token = await getValidGithubToken(env, userId);
      if (!token) return json({ error: "GitHub not connected" }, 404);
      return json({ token });
    }

    // ---- OAuth: disconnect ----
    if (path === "/oauth/disconnect" && method === "DELETE") {
      return handleOAuthDisconnect(request, env, json);
    }

    // ---- API keys: add ----
    if (path === "/apikeys/add" && method === "POST") {
      return handleAddApiKey(request, env, json);
    }

    // ---- API keys: remove ----
    if (path === "/apikeys/remove" && method === "DELETE") {
      return handleRemoveApiKey(request, env, json);
    }

    // ---- API keys: list ----
    if (path === "/apikeys/list" && method === "GET") {
      return handleListApiKeys(url, env, json);
    }

    // ---- Config check ----
    if (path === "/config/check" && method === "GET") {
      return json({
        google: { configured: !!env.GOOGLE_CLIENT_ID, scopes: GOOGLE_SCOPES },
        github: { configured: !!env.GITHUB_CLIENT_ID, scopes: GITHUB_SCOPES },
        redirect_uri: OAUTH_REDIRECT_URI,
      });
    }

    // ---- STT: Transcribe audio via Workers AI Whisper ----
    if (path === "/api/transcribe" && method === "POST") {
      try {
        const body = await request.json() as { audio_base64?: string };
        if (!body.audio_base64) return json({ error: "missing audio_base64" }, 400);
        const binaryStr = atob(body.audio_base64);
        const bytes = new Uint8Array(binaryStr.length);
        for (let i = 0; i < binaryStr.length; i++) {
          bytes[i] = binaryStr.charCodeAt(i);
        }
        const whisperResp = await env.AI.run("@cf/openai/whisper", {
          audio: [...bytes],
        });
        return json({ text: (whisperResp as any)?.text || "" });
      } catch (e) {
        return json({ error: (e as Error).message, text: "" }, 500);
      }
    }

    // ---- Main: process transcript ----
    if (path === "/" && method === "POST") {
      return handleTranscript(request, env, json);
    }

    return json({ error: "not found" }, 404);
  },
};

// ---- Registration handler ----

async function handleRegister(
  request: Request,
  env: Env,
  json: (d: unknown, s?: number) => Response,
): Promise<Response> {
  let body: any;
  try { body = await request.json(); } catch { return json({ error: "invalid JSON" }, 400); }

  const userId = body.user_id || "";
  const deviceId = body.device_id || "";
  if (!userId || !deviceId) return json({ error: "user_id and device_id required" }, 400);

  const now = Date.now() / 1000;
  await env.DB.prepare(
    "INSERT OR REPLACE INTO user_devices (user_id, device_id, device_name, os, device_token, created_at) VALUES (?, ?, ?, ?, ?, ?)"
  ).bind(userId, deviceId, body.device_name || "", body.os || "", body.device_token || null, now).run();

  return json({
    ok: true,
    user_id: userId,
    device_id: deviceId,
    server_config: {
      worker_url: new URL(request.url).origin,
      ws_url: "",  // no WebSocket — HTTP only
    },
    providers: {
      google: { configured: !!env.GOOGLE_CLIENT_ID, scopes: GOOGLE_SCOPES },
      github: { configured: !!env.GITHUB_CLIENT_ID, scopes: GITHUB_SCOPES },
    },
  });
}

// ---- OAuth auth URL handler ----

async function handleAuthUrl(
  url: URL,
  env: Env,
  json: (d: unknown, s?: number) => Response,
): Promise<Response> {
  const provider = (url.searchParams.get("provider") || "").toLowerCase();
  const userId = url.searchParams.get("user_id") || "";
  const codeChallenge = url.searchParams.get("code_challenge") || "";
  const workerOrigin = url.origin;
  const callbackUrl = `${workerOrigin}/oauth/callback`;

  // State format: provider:userId
  const state = `${provider}:${userId}`;

  if (provider === "google") {
    if (!env.GOOGLE_CLIENT_ID) return json({ error: "Google OAuth not configured" }, 500);
    const authUrl = (
      `https://accounts.google.com/o/oauth2/v2/auth`
      + `?client_id=${encodeURIComponent(env.GOOGLE_CLIENT_ID)}`
      + `&redirect_uri=${encodeURIComponent(callbackUrl)}`
      + `&response_type=code`
      + `&scope=${encodeURIComponent(GOOGLE_SCOPES)}`
      + (codeChallenge ? `&code_challenge=${encodeURIComponent(codeChallenge)}&code_challenge_method=S256` : "")
      + `&state=${encodeURIComponent(state)}`
      + `&access_type=offline`
      + `&prompt=consent`
    );
    return json({ url: authUrl, redirect_uri: callbackUrl });
  }

  if (provider === "github") {
    if (!env.GITHUB_CLIENT_ID) return json({ error: "GitHub OAuth not configured" }, 500);
    const authUrl = (
      `https://github.com/login/oauth/authorize`
      + `?client_id=${encodeURIComponent(env.GITHUB_CLIENT_ID)}`
      + `&redirect_uri=${encodeURIComponent(callbackUrl)}`
      + `&scope=${encodeURIComponent(GITHUB_SCOPES)}`
      + `&state=${encodeURIComponent(state)}`
    );
    return json({ url: authUrl, redirect_uri: callbackUrl });
  }

  return json({ error: `unsupported provider: ${provider}` }, 400);
}

function renderOAuthHtml(
  provider: string,
  success: boolean,
  errorMsg: string,
  userId: string,
  accountId = "",
): string {
  const providerDisplay = provider.toLowerCase() === "google" ? "Google" : provider.toLowerCase() === "github" ? "GitHub" : provider;
  const deepLink = `nexus://oauth/callback?provider=${encodeURIComponent(provider.toLowerCase())}&user_id=${encodeURIComponent(userId)}&status=${success ? "success" : "error"}`;

  if (!success) {
    return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>NEXUS - Connection Failed</title>
  <style>
    body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0f172a; color: #f8fafc; display: flex; align-items: center; justify-content: center; min-height: 100vh; margin: 0; padding: 20px; box-sizing: border-box; }
    .card { background: #1e293b; border: 1px solid #ef4444; border-radius: 16px; padding: 40px; text-align: center; max-width: 440px; width: 100%; box-shadow: 0 20px 25px -5px rgba(0,0,0,0.5); }
    .icon { width: 64px; height: 64px; background: rgba(239,68,68,0.2); color: #ef4444; border-radius: 50%; display: flex; align-items: center; justify-content: center; font-size: 32px; margin: 0 auto 20px; }
    h1 { font-size: 20px; margin: 0 0 10px; font-weight: 600; }
    p { color: #94a3b8; font-size: 14px; line-height: 1.5; margin: 0 0 24px; word-break: break-word; }
    .btn { display: inline-block; background: #3b82f6; color: white; padding: 10px 24px; border-radius: 8px; text-decoration: none; font-size: 14px; font-weight: 500; }
  </style>
</head>
<body>
  <div class="card">
    <div class="icon">✕</div>
    <h1>${providerDisplay} Connection Failed</h1>
    <p>${errorMsg || "Unable to complete authorization."}</p>
    <a href="${deepLink}" class="btn">Return to NEXUS</a>
  </div>
</body>
</html>`;
  }

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>NEXUS - ${providerDisplay} Connected</title>
  <style>
    body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #0f172a; color: #f8fafc; display: flex; align-items: center; justify-content: center; min-height: 100vh; margin: 0; padding: 20px; box-sizing: border-box; }
    .card { background: #1e293b; border: 1px solid #334155; border-radius: 16px; padding: 40px; text-align: center; max-width: 440px; width: 100%; box-shadow: 0 20px 25px -5px rgba(0,0,0,0.5); }
    .icon { width: 64px; height: 64px; background: rgba(34,197,94,0.2); color: #22c55e; border-radius: 50%; display: flex; align-items: center; justify-content: center; font-size: 32px; margin: 0 auto 20px; }
    h1 { font-size: 22px; margin: 0 0 10px; font-weight: 600; }
    p { color: #94a3b8; font-size: 14px; line-height: 1.5; margin: 0 0 24px; }
    .account { color: #60a5fa; font-weight: 600; }
    .btn { display: inline-block; background: #3b82f6; color: white; padding: 10px 24px; border-radius: 8px; text-decoration: none; font-size: 14px; font-weight: 500; }
  </style>
</head>
<body>
  <div class="card">
    <div class="icon">✓</div>
    <h1>${providerDisplay} Connected!</h1>
    <p>Your ${providerDisplay} account ${accountId ? `(<span class="account">${accountId}</span>)` : ""} is now connected to NEXUS. You can close this tab and return to the assistant.</p>
    <a href="${deepLink}" class="btn">Return to NEXUS</a>
  </div>
  <script>
    try {
      window.location.href = "${deepLink}";
    } catch(e) {}
    setTimeout(() => { try { window.close(); } catch(e) {} }, 3000);
  </script>
</body>
</html>`;
}

// ---- OAuth browser callback handler ----

async function handleOAuthBrowserCallback(
  url: URL,
  env: Env,
  json: (d: unknown, s?: number) => Response,
): Promise<Response> {
  const code = url.searchParams.get("code") || "";
  const state = url.searchParams.get("state") || "";
  const error = url.searchParams.get("error");
  const workerOrigin = url.origin;
  const callbackUrl = `${workerOrigin}/oauth/callback`;

  // Parse state: "provider:userId" or legacy "userId"
  let provider = "";
  let userId = state;
  if (state.includes(":")) {
    const parts = state.split(":");
    provider = parts[0].toLowerCase();
    userId = parts.slice(1).join(":");
  } else {
    provider = (url.searchParams.get("scope")?.includes("google") || url.searchParams.get("scope")?.includes("calendar")) ? "google" : "github";
  }

  if (error) {
    return new Response(renderOAuthHtml(provider || "Service", false, `Authorization error: ${error}`, userId), {
      headers: { "Content-Type": "text/html; charset=utf-8" },
    });
  }

  if (!code) {
    return new Response(renderOAuthHtml(provider || "Service", false, "Missing authorization code from provider.", userId), {
      headers: { "Content-Type": "text/html; charset=utf-8" },
    });
  }

  try {
    let accessToken = "";
    let refreshToken: string | null = null;
    let expiresIn = 0;
    let accountId = userId;

    if (provider === "google") {
      const resp = await fetch(GOOGLE_TOKEN_URL, {
        method: "POST",
        headers: { "Content-Type": "application/x-www-form-urlencoded" },
        body: new URLSearchParams({
          client_id: env.GOOGLE_CLIENT_ID,
          client_secret: env.GOOGLE_CLIENT_SECRET,
          code,
          redirect_uri: callbackUrl,
          grant_type: "authorization_code",
        }),
      });

      if (!resp.ok) {
        const errText = await resp.text();
        return new Response(renderOAuthHtml("Google", false, `Google token exchange failed: ${resp.status} ${errText}`, userId), {
          headers: { "Content-Type": "text/html; charset=utf-8" },
        });
      }

      const tokens = await resp.json() as any;
      accessToken = tokens.access_token;
      refreshToken = tokens.refresh_token || null;
      expiresIn = tokens.expires_in || 3600;

      // Fetch Google email
      try {
        const userInfoResp = await fetch("https://www.googleapis.com/oauth2/v2/userinfo", {
          headers: { "Authorization": `Bearer ${accessToken}` },
        });
        if (userInfoResp.ok) {
          const uInfo = await userInfoResp.json() as any;
          accountId = uInfo.email || userId;
        }
      } catch { /* ignore */ }

    } else if (provider === "github") {
      const resp = await fetch(GITHUB_TOKEN_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json", "Accept": "application/json" },
        body: JSON.stringify({
          client_id: env.GITHUB_CLIENT_ID,
          client_secret: env.GITHUB_CLIENT_SECRET,
          code,
          redirect_uri: callbackUrl,
        }),
      });

      if (!resp.ok) {
        const errText = await resp.text();
        return new Response(renderOAuthHtml("GitHub", false, `GitHub token exchange failed: ${resp.status} ${errText}`, userId), {
          headers: { "Content-Type": "text/html; charset=utf-8" },
        });
      }

      const tokens = await resp.json() as any;
      if (!tokens.access_token) {
        return new Response(renderOAuthHtml("GitHub", false, `GitHub exchange error: ${tokens.error_description || tokens.error || "No access token"}`, userId), {
          headers: { "Content-Type": "text/html; charset=utf-8" },
        });
      }

      accessToken = tokens.access_token;
      refreshToken = tokens.refresh_token || null;
      expiresIn = tokens.expires_in || 0;

      // Fetch GitHub login
      try {
        const ghResp = await fetch("https://api.github.com/user", {
          headers: { "Authorization": `Bearer ${accessToken}`, "User-Agent": "NEXUS-Worker" },
        });
        if (ghResp.ok) {
          const ghUser = await ghResp.json() as any;
          accountId = ghUser.login || userId;
        }
      } catch { /* ignore */ }
    } else {
      return new Response(renderOAuthHtml(provider || "Unknown", false, `Unsupported provider: ${provider}`, userId), {
        headers: { "Content-Type": "text/html; charset=utf-8" },
      });
    }

    // Save in Cloudflare D1
    const now = Date.now() / 1000;
    const expiresAt = expiresIn ? now + expiresIn : 0;
    const scopes = provider === "google" ? GOOGLE_SCOPES : GITHUB_SCOPES;

    await env.DB.prepare(
      "INSERT OR REPLACE INTO oauth_tokens (user_id, provider, access_token, refresh_token, expires_at, scopes, account_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    ).bind(
      userId, provider, accessToken,
      refreshToken, expiresAt, scopes, accountId, now
    ).run();

    return new Response(renderOAuthHtml(provider === "google" ? "Google" : "GitHub", true, "", userId, accountId), {
      headers: { "Content-Type": "text/html; charset=utf-8" },
    });
  } catch (err) {
    return new Response(renderOAuthHtml(provider || "Service", false, `OAuth Error: ${(err as Error).message}`, userId), {
      headers: { "Content-Type": "text/html; charset=utf-8" },
    });
  }
}

// ---- OAuth exchange handler ----

async function handleOAuthExchange(
  request: Request,
  env: Env,
  json: (d: unknown, s?: number) => Response,
): Promise<Response> {
  let body: any;
  try { body = await request.json(); } catch { return json({ error: "invalid JSON" }, 400); }

  const provider = (body.provider || "").toLowerCase();
  const code = body.code || "";
  const codeVerifier = body.code_verifier || "";
  const userId = body.user_id || "";
  const workerOrigin = new URL(request.url).origin;
  const redirectUri = body.redirect_uri || `${workerOrigin}/oauth/callback`;

  if (!provider || !code || !userId) return json({ error: "missing required fields" }, 400);

  try {
    let tokens: any;
    let accountId = userId;

    if (provider === "google") {
      const resp = await fetch(GOOGLE_TOKEN_URL, {
        method: "POST",
        headers: { "Content-Type": "application/x-www-form-urlencoded" },
        body: new URLSearchParams({
          client_id: env.GOOGLE_CLIENT_ID,
          client_secret: env.GOOGLE_CLIENT_SECRET,
          code,
          code_verifier: codeVerifier,
          redirect_uri: redirectUri,
          grant_type: "authorization_code",
        }),
      });
      if (!resp.ok) return json({ error: `Google exchange failed (${resp.status})` }, 502);
      tokens = await resp.json();

      try {
        const userInfoResp = await fetch("https://www.googleapis.com/oauth2/v2/userinfo", {
          headers: { "Authorization": `Bearer ${tokens.access_token}` },
        });
        if (userInfoResp.ok) {
          const uInfo = await userInfoResp.json() as any;
          accountId = uInfo.email || userId;
        }
      } catch { /* ignore */ }
    } else if (provider === "github") {
      const resp = await fetch(GITHUB_TOKEN_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json", "Accept": "application/json" },
        body: JSON.stringify({
          client_id: env.GITHUB_CLIENT_ID,
          client_secret: env.GITHUB_CLIENT_SECRET,
          code,
          code_verifier: codeVerifier,
          redirect_uri: redirectUri,
        }),
      });
      if (!resp.ok) return json({ error: `GitHub exchange failed (${resp.status})` }, 502);
      tokens = await resp.json();
      if (!tokens.access_token) return json({ error: tokens.error_description || tokens.error || "exchange failed" }, 400);

      try {
        const ghResp = await fetch("https://api.github.com/user", {
          headers: { "Authorization": `Bearer ${tokens.access_token}`, "User-Agent": "NEXUS-Worker" },
        });
        if (ghResp.ok) {
          const ghUser = await ghResp.json() as any;
          accountId = ghUser.login || userId;
        }
      } catch { /* ignore */ }
    } else {
      return json({ error: `unsupported provider: ${provider}` }, 400);
    }

    // Store in D1
    const now = Date.now() / 1000;
    const expiresAt = tokens.expires_in ? now + tokens.expires_in : 0;

    await env.DB.prepare(
      "INSERT OR REPLACE INTO oauth_tokens (user_id, provider, access_token, refresh_token, expires_at, scopes, account_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    ).bind(
      userId, provider, tokens.access_token,
      tokens.refresh_token || null, expiresAt,
      provider === "google" ? GOOGLE_SCOPES : GITHUB_SCOPES,
      accountId, now
    ).run();

    return json({ ok: true, provider, connected: true, account_id: accountId });
  } catch (err) {
    return json({ error: `exchange error: ${(err as Error).message}` }, 500);
  }
}

// ---- OAuth status handler ----

async function handleOAuthStatus(
  url: URL,
  env: Env,
  json: (d: unknown, s?: number) => Response,
): Promise<Response> {
  const userId = url.searchParams.get("user_id") || "";
  if (!userId) return json({ error: "user_id required" }, 400);

  const connected: Record<string, any> = {};
  const now = Date.now() / 1000;
  // We need refresh_token to determine if an expired token can be refreshed
  const refreshResult = await env.DB.prepare(
    "SELECT provider, expires_at, scopes, refresh_token FROM oauth_tokens WHERE user_id = ?"
  ).bind(userId).all();

  for (const row of refreshResult.results || []) {
    const expiresAt = row.expires_at as number;
    const hasRefresh = !!row.refresh_token;
    // Classic tokens (expires_at = 0) never expire.
    // GitHub App tokens with a refresh_token can be refreshed even if
    // expires_at has passed, so they are NOT reported as expired.
    // Only report expired if the token has expired AND there is no
    // refresh_token to renew it.
    const isExpired = expiresAt ? (now > expiresAt && !hasRefresh) : false;
    connected[row.provider as string] = {
      connected: true,
      expired: isExpired,
      scopes: row.scopes as string,
    };
  }
  return json({ user_id: userId, providers: connected });
}

// ---- OAuth disconnect handler ----

async function handleOAuthDisconnect(
  request: Request,
  env: Env,
  json: (d: unknown, s?: number) => Response,
): Promise<Response> {
  let body: any;
  try { body = await request.json(); } catch { return json({ error: "invalid JSON" }, 400); }
  const userId = body.user_id || "";
  const provider = body.provider || "";
  if (!userId || !provider) return json({ error: "user_id and provider required" }, 400);

  await env.DB.prepare(
    "DELETE FROM oauth_tokens WHERE user_id = ? AND provider = ?"
  ).bind(userId, provider).run();

  return json({ ok: true, disconnected: provider });
}

// ---- API key handlers ----

async function handleAddApiKey(
  request: Request,
  env: Env,
  json: (d: unknown, s?: number) => Response,
): Promise<Response> {
  let body: any;
  try { body = await request.json(); } catch { return json({ error: "invalid JSON" }, 400); }
  const userId = body.user_id || "";
  const provider = body.provider || "";
  const apiKey = body.api_key || "";
  if (!userId || !provider || !apiKey) return json({ error: "missing required fields" }, 400);

  // Simple obfuscation (not real encryption in Worker — D1 is already encrypted at rest)
  const encrypted = btoa(apiKey);
  const now = Date.now() / 1000;
  await env.DB.prepare(
    "INSERT OR REPLACE INTO api_keys (user_id, provider, key_encrypted, created_at) VALUES (?, ?, ?, ?)"
  ).bind(userId, provider, encrypted, now).run();

  return json({ ok: true, provider, stored: true });
}

async function handleRemoveApiKey(
  request: Request,
  env: Env,
  json: (d: unknown, s?: number) => Response,
): Promise<Response> {
  let body: any;
  try { body = await request.json(); } catch { return json({ error: "invalid JSON" }, 400); }
  await env.DB.prepare(
    "DELETE FROM api_keys WHERE user_id = ? AND provider = ?"
  ).bind(body.user_id || "", body.provider || "").run();
  return json({ ok: true, removed: body.provider });
}

async function handleListApiKeys(
  url: URL,
  env: Env,
  json: (d: unknown, s?: number) => Response,
): Promise<Response> {
  const userId = url.searchParams.get("user_id") || "";
  if (!userId) return json({ error: "user_id required" }, 400);
  const result = await env.DB.prepare(
    "SELECT provider FROM api_keys WHERE user_id = ?"
  ).bind(userId).all();
  return json({ user_id: userId, providers: (result.results || []).map(r => r.provider) });
}

// ---- Main transcript handler ----

async function handleTranscript(
  request: Request,
  env: Env,
  json: (d: unknown, s?: number) => Response,
): Promise<Response> {
  let req: NexusRequest;
  try { req = await request.json() as NexusRequest; } catch {
    return json({ error: "invalid JSON" }, 400);
  }

  if (!req.task?.request) return json({ error: "missing task.request" }, 400);

  const userId = req.requester?.id || "";
  if (!userId) return json({ error: "missing requester.id" }, 400);

  // ─── Dialog context follow-up ──────────────────────────────────────
  // If the client sent a dialog_context (from a previous follow-up question),
  // combine the original request with the new input so the intent classifier
  // sees the full command (e.g. "analyse" + "zync" → "analyse zync").
  const dialogCtx = req.task.dialog_context;
  if (dialogCtx && dialogCtx.pending_intent && dialogCtx.original_request) {
    const combined = `${dialogCtx.original_request} ${req.task.request}`.trim();
    console.log(`[dialog] follow-up: combining "${dialogCtx.original_request}" + "${req.task.request}" → "${combined}"`);
    req.task.request = combined;
  }

  // 1. Classify intent — but allow explicit intent override from the task
  // (e.g. architect sidebar sends intent="impact_narration" directly)
  const explicitIntent = (req.task as any)?.intent;
  let intent = explicitIntent || await classifyIntent(req.task.request, env);

  // 1b. If intent is "general" but the transcript looks like a factual question,
  // route to "search" so it goes through Wikipedia/Wikidata retrieval.
  // But skip this for greetings/identity questions ("who are you", "thank you")
  // — those should stay "general" and get a conversational response.
  if (intent === "general" && isSearchQuestion(req.task.request) && !isGreetingOrThanks(req.task.request)) {
    intent = "search";
  }

  // 1c. Quota check — reject if user has exceeded daily limits
  const isDeep = intent === "github_analyse" || intent === "deep_analyse";
  const isSearch = intent === "search";
  const quota = await checkQuota(env, userId, isDeep, isSearch);
  if (!quota.allowed) {
    return json({
      request_id: req.request_id,
      reply_text: quota.reason || "Daily limit reached.",
      intent,
      quota_exceeded: true,
    });
  }

  // 2. Get credentials from D1 based on intent
  let replyText: string;
  let analysisData: any = null;
  let dialogState: DialogState | null = null;

  try {
    if (intent === "analyze_repo") {
      replyText = await handleAnalyzeRepo(req, env);
    } else if (intent === "fast_analyse") {
      const token = await getValidGithubToken(env, userId);
      if (!token) {
        replyText = "You haven't connected your GitHub account yet. Please connect it in the NEXUS setup to analyse repositories.";
      } else {
        const result = await handleFastAnalyse(req, env, token);
        replyText = result.text;
        if (result.analysis) {
          analysisData = result.analysis;
        }
        // If the repo name was missing or couldn't be resolved, ask a follow-up
        if (replyText.includes("Which repository would you like me to analyse?")
            || (replyText.includes("couldn't find a repository matching") && !req.task.dialog_context)) {
          dialogState = {
            expects_followup: true,
            pending_intent: "fast_analyse",
            context: {
              pending_intent: "fast_analyse",
              original_request: req.task.request,
              missing: ["repo"],
            },
          };
        }
      }
    } else if (intent === "deep_analyse") {
      // Deep analyse triggers the client-side architect window with clone + AST
      replyText = "Opening the architecture mapper for a deep scan, sir. This will clone the repository and build a full dependency graph. It may take 30 to 60 seconds.";
    } else if (intent === "phase1_enrich") {
      replyText = await handlePhase1Enrich(req, env);
    } else if (intent === "impact_narration") {
      replyText = await handleImpactNarration(req, env);
    } else if (intent === "github_analyse") {
      const token = await getValidGithubToken(env, userId);
      if (!token) {
        replyText = "You haven't connected your GitHub account yet. Please connect it in the NEXUS setup to analyse PRs.";
      } else {
        replyText = await handleGitHubAnalyse(req, env, token);
        // If the repo couldn't be resolved and the message asks for a repo,
        // set dialog state for follow-up
        if (replyText.includes("couldn't find a repository matching") && !req.task.dialog_context) {
          dialogState = {
            expects_followup: true,
            pending_intent: "github_analyse",
            context: {
              pending_intent: "github_analyse",
              original_request: req.task.request,
              missing: ["repo"],
            },
          };
        }
      }
    } else if (intent === "github") {
      const token = await getValidGithubToken(env, userId);
      if (!token) {
        replyText = "You haven't connected your GitHub account yet. Please connect it in the NEXUS setup.";
      } else {
        replyText = await handleGitHub(req, env, token);
      }
    } else if (intent === "github_write") {
      const token = await getValidGithubToken(env, userId);
      if (!token) {
        replyText = "You haven't connected your GitHub account yet. Please connect it in the NEXUS setup.";
      } else {
        replyText = await handleGitHubWrite(req, env, token);
      }
    } else if (intent === "gmail") {
      const token = await getValidGoogleToken(env, userId);
      if (!token) {
        replyText = "You haven't connected your Google account yet. Please connect it in the NEXUS setup.";
      } else {
        replyText = await handleGmail(req, env, token);
      }
    } else if (intent === "calendar") {
      const token = await getValidGoogleToken(env, userId);
      if (!token) {
        replyText = "You haven't connected your Google account yet. Please connect it in the NEXUS setup.";
      } else {
        replyText = await handleCalendar(req, env, token);
      }
    } else if (intent === "search") {
      replyText = await handleSearch(req, env);
    } else {
      replyText = await handleGeneral(req, env);
    }
  } catch (err) {
    replyText = `I ran into an error processing that request: ${(err as Error).message}`;
  }

  // Track usage (non-blocking — don't fail the request if tracking fails)
  try {
    const neurons = isDeep ? 500 : isSearch ? 100 : 50;
    await incrementUsage(env, userId, {
      requests: 1,
      ai_neurons: neurons,
      deep_calls: isDeep ? 1 : 0,
      search_calls: isSearch ? 1 : 0,
    });
  } catch { /* quota tracking is best-effort */ }

  if (analysisData) {
    const resp: any = { request_id: req.request_id, reply_text: replyText, intent, analysis: analysisData };
    if (dialogState) resp.dialog_state = dialogState;
    return json(resp);
  }
  const resp: any = { request_id: req.request_id, reply_text: replyText, intent };
  if (dialogState) resp.dialog_state = dialogState;
  return json(resp);
}

// ---- Architecture Mapper Intent Handler ----

async function handleAnalyzeRepo(req: NexusRequest, env: Env): Promise<string> {
  // The architect window is opened client-side by wsBridge when it detects
  // this is an architect query. The Worker just returns a short spoken
  // confirmation that NEXUS says aloud while the architect window loads.
  return "Opening the architecture mapper, sir. I'll analyze the repository structure, build a real dependency graph, and have it ready for you to explore.";
}

// ---- Fast Repo Analyse (sidebar, no clone) ----
//
// "NEXUS, analyse eesh264/congi" → uses GitHub OAuth token to fetch
// repo metadata + file tree + key file contents + languages, then uses
// GLM-4.7-flash (free) to generate a rich analysis. Shows in the sidebar
// with pie charts for languages and frameworks. Works with both public
// and private repos (using the user's token).
//
// Returns a structured object with both spoken text and analysis data.

interface RepoAnalysis {
  repo: string;
  visibility: string;
  description: string;
  stars: number;
  forks: number;
  totalFiles: number;
  languages: { name: string; bytes: number; percentage: number }[];
  frameworks: { name: string; category: string }[];
  databases: { name: string; evidence: string }[];
  features: string[];
  tests: boolean;
  ci: string;
  docker: boolean;
  architecture: string;
  defaultBranch: string;
}

async function handleFastAnalyse(req: NexusRequest, env: Env, token: string): Promise<{ text: string; analysis: RepoAnalysis | null }> {
  const transcript = req.task.request;
  const userId = req.requester.id;

  // Parse repo name from transcript: "analyse owner/repo", "analyse repo owner/repo",
  // "analyse repoName", or "analyse repo repoName"
  // The optional "repo" word between the verb and the name must be skipped.
  const analyseMatch = transcript.match(/analy[sz]e\s+(?:repo\s+)?([a-zA-Z0-9_.\-]+\/[a-zA-Z0-9_.\-]+)/i);
  let repoName: string | null = null;

  if (analyseMatch) {
    repoName = analyseMatch[1];
  } else {
    // Single-name fallback: "analyse repoName" or "analyse repo repoName"
    // Skip the word "repo" if it appears right after "analyse"
    const singleMatch = transcript.match(/analy[sz]e\s+(?:repo\s+)?([a-zA-Z0-9_.\-]+)/i);
    if (singleMatch) {
      repoName = singleMatch[1];
    }
  }

  if (!repoName) {
    return { text: "Which repository would you like me to analyse? Say something like 'analyse owner/repo'.", analysis: null };
  }

  const authHeaders: Record<string, string> = {
    "Authorization": `Bearer ${token}`,
    "Accept": "application/vnd.github+json",
    "User-Agent": "NEXUS-Worker",
  };
  const noAuthHeaders: Record<string, string> = {
    "Accept": "application/vnd.github+json",
    "User-Agent": "NEXUS-Worker",
  };

  // Resolve repo name (if no owner/, search user's repos)
  let fullRepo = repoName;
  if (!repoName.includes("/")) {
    const resolved = await resolveRepo(token, repoName);
    if (!resolved.full_name) {
      const repoList = resolved.availableRepos.length > 0
        ? `\n\nYour available repositories: ${resolved.availableRepos.join(", ")}`
        : "";
      return {
        text: `I couldn't find a repository matching "${repoName}" in your GitHub account. Your GitHub token may have expired — please reconnect GitHub in the NEXUS setup. Alternatively, specify the full name like "analyse owner/${repoName}".${repoList}`,
        analysis: null,
      };
    }
    fullRepo = resolved.full_name;
  }

  try {
    // ─── Step 1: Fetch repo metadata (try with token, fall back to no auth) ─
    let metaResp = await fetch(`https://api.github.com/repos/${fullRepo}`, { headers: authHeaders });
    let usingAuth = true;
    if (metaResp.status === 401) {
      metaResp = await fetch(`https://api.github.com/repos/${fullRepo}`, { headers: noAuthHeaders });
      usingAuth = false;
    }

    if (!metaResp.ok) {
      if (metaResp.status === 404) {
        return {
          text: `I couldn't find the repository "${fullRepo}", sir. It might not exist, or it's private and your GitHub token has been revoked. Please reconnect GitHub in the NEXUS setup wizard.`,
          analysis: null,
        };
      }
      return { text: githubErrorMessage(metaResp.status, `analyse ${fullRepo}`), analysis: null };
    }

    const meta = await metaResp.json() as Record<string, any>;
    const isPrivate = meta.private === true;
    const description = meta.description || "No description provided.";
    const language = meta.language || "Unknown";
    const defaultBranch = meta.default_branch || "main";
    const stars = meta.stargazers_count || 0;
    const forks = meta.forks_count || 0;

    const reqHeaders = usingAuth ? authHeaders : noAuthHeaders;

    // ─── Step 2: Fetch file tree, languages, and topics in parallel ──────
    const [treeResp, langResp, topicsResp] = await Promise.all([
      fetch(`https://api.github.com/repos/${fullRepo}/git/trees/HEAD?recursive=1`, { headers: reqHeaders }),
      fetch(`https://api.github.com/repos/${fullRepo}/languages`, { headers: reqHeaders }),
      fetch(`https://api.github.com/repos/${fullRepo}/topics`, { headers: { ...reqHeaders, "Accept": "application/vnd.github.mercy-preview+json" } }),
    ]);

    let filePaths: string[] = [];
    if (treeResp.ok) {
      const tree = await treeResp.json() as Record<string, any>;
      if (Array.isArray(tree.tree)) {
        filePaths = tree.tree
          .filter((item: any) => item.type === "blob")
          .map((item: any) => item.path as string);
      }
    }

    // Language byte counts from GitHub API
    let languages: { name: string; bytes: number; percentage: number }[] = [];
    if (langResp.ok) {
      const langData = await langResp.json() as Record<string, number>;
      const totalBytes = Object.values(langData).reduce((a, b) => a + b, 0);
      languages = Object.entries(langData)
        .map(([name, bytes]) => ({
          name,
          bytes,
          percentage: totalBytes > 0 ? Math.round((bytes / totalBytes) * 1000) / 10 : 0,
        }))
        .sort((a, b) => b.bytes - a.bytes);
    }

    // Topics
    let topics: string[] = [];
    if (topicsResp.ok) {
      const topicsData = await topicsResp.json() as Record<string, any>;
      topics = Array.isArray(topicsData.names) ? topicsData.names : [];
    }

    const totalFiles = filePaths.length;

    // ─── Step 3: Fetch key file contents ────────────────────────────
    const keyFilePatterns = [
      "README.md", "package.json", "Cargo.toml", "pyproject.toml",
      "requirements.txt", "go.mod", "tsconfig.json", "vite.config.ts",
      "Dockerfile", "docker-compose.yml", ".github/workflows/ci.yml",
      "src/main.tsx", "src/main.ts", "src/main.rs", "src/lib.rs",
      "main.go", "app.py", "src/app.py",
    ];

    const fileSet = new Set(filePaths);
    const filesToFetch = keyFilePatterns.filter(f => fileSet.has(f)).slice(0, 8);

    const keyFileResults = await Promise.all(
      filesToFetch.map(async (path) => {
        try {
          const resp = await fetch(
            `https://api.github.com/repos/${fullRepo}/contents/${path}`,
            { headers: reqHeaders }
          );
          if (!resp.ok) return null;
          const data = await resp.json() as Record<string, any>;
          const size = data.size || 0;
          if (size > 50000) return { path, content: `[File too large: ${Math.round(size/1024)}KB]` };
          const encoded = data.content || "";
          const decoded = atob(encoded.replace(/\n/g, "").replace(/\r/g, ""));
          const content = decoded.length > 10000 ? decoded.substring(0, 10000) + "\n...[truncated]" : decoded;
          return { path, content };
        } catch {
          return null;
        }
      })
    );

    const keyFiles = keyFileResults.filter((f): f is {path: string, content: string} => f !== null);

    // ─── Step 4: Detect tech stack ──────────────────────────────────
    const frameworks: { name: string; category: string }[] = [];
    const databases: { name: string; evidence: string }[] = [];
    let buildTool = "unknown";
    const hasTests = filePaths.some(p => /test|spec|__tests__/i.test(p));
    const hasCI = filePaths.some(p => p.includes(".github/workflows"));
    const hasDocker = filePaths.some(p => /dockerfile|docker-compose/i.test(p));

    // Database detection patterns
    const dbPatterns: Record<string, string[]> = {
      "MongoDB": ["mongoose", "mongodb", "@prisma/client", "mongo"],
      "PostgreSQL": ["pg", "postgres", "psycopg2", "sqlalchemy", "prisma", "diesel", "sqlx", "gorm", "pgx"],
      "MySQL": ["mysql2", "mysql", "sequelize", "typeorm", "go-sql-driver"],
      "Redis": ["redis", "ioredis", "bull", "sidekiq", "celery"],
      "SQLite": ["sqlite", "better-sqlite3", "sql.js"],
      "Supabase": ["supabase", "@supabase/supabase-js"],
      "Firebase": ["firebase", "firestore", "@firebase"],
      "Prisma": ["prisma", "@prisma/client"],
      "Drizzle": ["drizzle-orm", "drizzle-kit"],
    };

    for (const kf of keyFiles) {
      if (kf.path === "package.json") {
        try {
          const pkg = JSON.parse(kf.content);
          const deps = { ...pkg.dependencies, ...pkg.devDependencies };
          // Frameworks
          if (deps.next) frameworks.push({ name: "Next.js", category: "frontend" });
          if (deps.react) frameworks.push({ name: "React", category: "frontend" });
          if (deps.vue) frameworks.push({ name: "Vue", category: "frontend" });
          if (deps.svelte) frameworks.push({ name: "Svelte", category: "frontend" });
          if (deps.express) frameworks.push({ name: "Express", category: "backend" });
          if (deps.fastify) frameworks.push({ name: "Fastify", category: "backend" });
          if (deps.nestjs || deps["@nestjs/core"]) frameworks.push({ name: "NestJS", category: "backend" });
          if (deps["@tauri-apps/api"]) frameworks.push({ name: "Tauri", category: "desktop" });
          if (deps.electron) frameworks.push({ name: "Electron", category: "desktop" });
          if (deps.tailwindcss) frameworks.push({ name: "Tailwind CSS", category: "styling" });
          if (deps.styled) frameworks.push({ name: "Styled Components", category: "styling" });
          if (deps.redux || deps["@reduxjs/toolkit"]) frameworks.push({ name: "Redux", category: "state" });
          if (deps.zustand) frameworks.push({ name: "Zustand", category: "state" });
          if (deps.jest) frameworks.push({ name: "Jest", category: "testing" });
          if (deps.vitest) frameworks.push({ name: "Vitest", category: "testing" });
          if (deps["@testing-library/react"]) frameworks.push({ name: "Testing Library", category: "testing" });
          if (deps.vite || pkg.devDependencies?.vite) { frameworks.push({ name: "Vite", category: "build" }); buildTool = "Vite"; }
          if (deps.webpack || pkg.devDependencies?.webpack) { buildTool = "Webpack"; frameworks.push({ name: "Webpack", category: "build" }); }
          if (deps.typescript) frameworks.push({ name: "TypeScript", category: "language" });
          // Databases
          for (const [dbName, patterns] of Object.entries(dbPatterns)) {
            const matchedPattern = patterns.find(pat => deps[pat] || deps[`@${pat}`]);
            if (matchedPattern) {
              if (!databases.find(d => d.name === dbName)) {
                databases.push({ name: dbName, evidence: `${matchedPattern} in package.json` });
              }
            }
          }
        } catch {}
      } else if (kf.path === "Cargo.toml") {
        frameworks.push({ name: "Rust", category: "language" });
        if (kf.content.includes("tauri")) frameworks.push({ name: "Tauri", category: "desktop" });
        else if (kf.content.includes("actix")) frameworks.push({ name: "Actix Web", category: "backend" });
        else if (kf.content.includes("axum")) frameworks.push({ name: "Axum", category: "backend" });
        else if (kf.content.includes("rocket")) frameworks.push({ name: "Rocket", category: "backend" });
        if (kf.content.includes("tokio")) frameworks.push({ name: "Tokio", category: "runtime" });
        if (kf.content.includes("serde")) frameworks.push({ name: "Serde", category: "serialization" });
        buildTool = "cargo";
        // Databases
        for (const [dbName, patterns] of Object.entries(dbPatterns)) {
          const matchedPattern = patterns.find(pat => kf.content.includes(pat));
          if (matchedPattern) {
            if (!databases.find(d => d.name === dbName)) {
              databases.push({ name: dbName, evidence: `${matchedPattern} in Cargo.toml` });
            }
          }
        }
      } else if (kf.path === "pyproject.toml" || kf.path === "requirements.txt") {
        if (kf.content.includes("fastapi")) frameworks.push({ name: "FastAPI", category: "backend" });
        if (kf.content.includes("flask")) frameworks.push({ name: "Flask", category: "backend" });
        if (kf.content.includes("django")) frameworks.push({ name: "Django", category: "backend" });
        if (kf.content.includes("streamlit")) frameworks.push({ name: "Streamlit", category: "frontend" });
        if (kf.content.includes("pytest")) frameworks.push({ name: "pytest", category: "testing" });
        buildTool = kf.path === "pyproject.toml" ? "poetry" : "pip";
        for (const [dbName, patterns] of Object.entries(dbPatterns)) {
          const matchedPattern = patterns.find(pat => kf.content.includes(pat));
          if (matchedPattern) {
            if (!databases.find(d => d.name === dbName)) {
              databases.push({ name: dbName, evidence: `${matchedPattern} in ${kf.path}` });
            }
          }
        }
      } else if (kf.path === "go.mod") {
        frameworks.push({ name: "Go", category: "language" });
        if (kf.content.includes("gin-gonic")) frameworks.push({ name: "Gin", category: "backend" });
        if (kf.content.includes("fiber")) frameworks.push({ name: "Fiber", category: "backend" });
        if (kf.content.includes("echo")) frameworks.push({ name: "Echo", category: "backend" });
        buildTool = "go";
        for (const [dbName, patterns] of Object.entries(dbPatterns)) {
          const matchedPattern = patterns.find(pat => kf.content.includes(pat));
          if (matchedPattern) {
            if (!databases.find(d => d.name === dbName)) {
              databases.push({ name: dbName, evidence: `${matchedPattern} in go.mod` });
            }
          }
        }
      } else if (kf.path === "docker-compose.yml") {
        // Detect database services in docker-compose
        const content = kf.content.toLowerCase();
        if (content.includes("postgres") || content.includes("postgresql")) {
          if (!databases.find(d => d.name === "PostgreSQL")) databases.push({ name: "PostgreSQL", evidence: "docker-compose.yml" });
        }
        if (content.includes("mysql") || content.includes("mariadb")) {
          if (!databases.find(d => d.name === "MySQL")) databases.push({ name: "MySQL", evidence: "docker-compose.yml" });
        }
        if (content.includes("redis")) {
          if (!databases.find(d => d.name === "Redis")) databases.push({ name: "Redis", evidence: "docker-compose.yml" });
        }
        if (content.includes("mongo")) {
          if (!databases.find(d => d.name === "MongoDB")) databases.push({ name: "MongoDB", evidence: "docker-compose.yml" });
        }
      }
    }

    // Check for Prisma schema
    if (filePaths.some(p => p.includes("prisma/schema.prisma"))) {
      if (!databases.find(d => d.name === "Prisma")) {
        databases.push({ name: "Prisma", evidence: "prisma/schema.prisma" });
      }
    }

    // ─── Step 5: Extract features from README ───────────────────────
    let features: string[] = [];
    const readme = keyFiles.find(kf => kf.path === "README.md");
    if (readme) {
      // Extract bullet points from Features/Features section
      const lines = readme.content.split("\n");
      let inFeaturesSection = false;
      for (const line of lines) {
        const headingMatch = line.match(/^#+\s*(features?|key features?|what it does|capabilities)/i);
        if (headingMatch) {
          inFeaturesSection = true;
          continue;
        }
        if (inFeaturesSection) {
          // Stop at next heading
          if (/^#+\s/.test(line)) {
            inFeaturesSection = false;
            continue;
          }
          // Extract bullet points
          const bulletMatch = line.match(/^\s*[-*+]\s+(.+)/);
          if (bulletMatch) {
            const feature = bulletMatch[1].replace(/\*\*(.+?)\*\*/g, "$1").replace(/\[(.+?)\]\(.+?\)/g, "$1").trim();
            if (feature.length > 3 && feature.length < 100) {
              features.push(feature);
            }
          }
        }
      }
    }
    // Add topics as features if we don't have enough
    if (features.length < 3 && topics.length > 0) {
      features = topics.slice(0, 8).map(t => t.replace(/-/g, " "));
    }

    // ─── Step 6: Build LLM prompt for GLM-4.7-flash ──────────────────
    const privacyNote = isPrivate ? " (private repository)" : " (public repository)";
    const fileTreeSample = filePaths.slice(0, 200).join("\n");
    const keyFilesText = keyFiles.map(kf => `--- ${kf.path} ---\n${kf.content}`).join("\n\n");
    const langText = languages.map(l => `${l.name}: ${l.percentage}%`).join(", ");
    const fwText = frameworks.map(f => `${f.name} (${f.category})`).join(", ");
    const dbText = databases.map(d => `${d.name} (${d.evidence})`).join(", ");

    const prompt = `You are NEXUS, an AI assistant. The user asked you to analyse a GitHub repository. Provide a natural spoken summary.

Repository: ${fullRepo}${privacyNote}
Description: ${description}
Languages: ${langText}
Frameworks: ${fwText}
Databases: ${dbText}
Stars: ${stars} | Forks: ${forks}
Total files: ${totalFiles}
Build tool: ${buildTool}
Tests: ${hasTests ? "yes" : "no"} | CI: ${hasCI ? "yes" : "no"} | Docker: ${hasDocker ? "yes" : "no"}
Topics: ${topics.join(", ")}

File tree (first 200 files):
${fileTreeSample}

Key file contents:
${keyFilesText}

Write a natural spoken summary (max 100 words) covering:
1. What the project does
2. Tech stack (languages, frameworks, databases)
3. Architecture overview
4. Notable aspects (tests, CI, Docker, scale)

Speak naturally as NEXUS addressing the user as "sir". Start with "Ok sir, " for public repos or "Ok sir, I've accessed your private repository. " for private repos. Output ONLY the spoken summary text — no JSON, no reasoning, no markdown, no headers.`;

    // ─── Step 7: Generate analysis via GLM-4.7-flash ─────────────────
    let spokenSummary: string;
    let architectureSummary = "";

    try {
      // Use mistral for spoken summary — no reasoning leakage, clean output
      const response = await env.AI.run(SUMMARY_MODEL as any, {
        messages: [{ role: "user", content: prompt }],
        max_tokens: 300,
      });
      spokenSummary = extractText(response);
      // Clean up: if the model includes reasoning, take only from "Ok sir"
      if (spokenSummary.includes("Ok sir")) {
        const idx = spokenSummary.indexOf("Ok sir");
        spokenSummary = spokenSummary.substring(idx);
      }
      // If no "Ok sir" prefix, add it
      if (!spokenSummary.startsWith("Ok sir")) {
        spokenSummary = `Ok sir, ${spokenSummary}`;
      }
    } catch {
      // Fallback: heuristic summary without LLM
      spokenSummary = `Ok sir, ${fullRepo} is a ${language} repository${privacyNote}. ${description} It has ${totalFiles} files, built with ${frameworks.map(f => f.name).join(", ") || "unknown framework"} and ${buildTool}.`;
      if (databases.length > 0) spokenSummary += ` Uses ${databases.map(d => d.name).join(" and ")} for data storage.`;
      if (!hasTests) spokenSummary += " No tests were found.";
      if (!hasCI) spokenSummary += " No CI/CD pipelines detected.";
      if (!hasDocker) spokenSummary += " No Docker setup found.";
    }

    // Use extracted features from README + topics as the feature list
    const finalFeatures = features.length > 0 ? features : topics.slice(0, 8).map(t => t.replace(/-/g, " "));

    // Build architecture summary if LLM didn't provide one
    if (!architectureSummary) {
      const parts: string[] = [];
      const frontend = frameworks.filter(f => f.category === "frontend").map(f => f.name);
      const backend = frameworks.filter(f => f.category === "backend").map(f => f.name);
      if (frontend.length) parts.push(`Frontend (${frontend.join("/")})`);
      if (backend.length) parts.push(`Backend (${backend.join("/")})`);
      if (databases.length) parts.push(`Database (${databases.map(d => d.name).join("/")})`);
      architectureSummary = parts.join(" + ") || `${language} application`;
    }

    // ─── Step 8: Build structured analysis object ────────────────────
    const analysis: RepoAnalysis = {
      repo: fullRepo,
      visibility: isPrivate ? "private" : "public",
      description,
      stars,
      forks,
      totalFiles,
      languages,
      frameworks: frameworks.length > 0 ? frameworks : [{ name: language, category: "language" }],
      databases,
      features: finalFeatures,
      tests: hasTests,
      ci: hasCI ? "GitHub Actions" : "none",
      docker: hasDocker,
      architecture: architectureSummary,
      defaultBranch,
    };

    return { text: spokenSummary, analysis };

  } catch (err) {
    return { text: `I ran into an error analysing ${fullRepo}: ${(err as Error).message}`, analysis: null };
  }
}

// ---- Architecture Mapper: Phase 1 LLM Enrichment ----
// Called by the Rust client AFTER the instant heuristic-based Phase 1 diagram
// is already shown to the user. The LLM enriches the generic layer labels
// (e.g. "Client / Presentation Layer") with repo-specific intelligence
// (e.g. "Next.js App Router — React 19 SSR pages") and writes a real summary.
// This never blocks first paint — it streams in ~2-3s after the diagram appears.

async function handlePhase1Enrich(req: NexusRequest, env: Env): Promise<string> {
  const payload = req.task as any;
  const owner: string = payload.owner || "";
  const repo: string = payload.repo || "";
  const primary_language: string = payload.primary_language || "TypeScript";
  const description: string = payload.description || "";
  const total_files: number = payload.total_files || 0;

  // The Rust heuristic layers — the LLM rewrites labels/tech_stack per layer
  const layers: Array<{
    id: string; label: string; layer_type: string;
    dirs: string[]; tech_stack: string; file_count: number;
    sample_files: string[];
  }> = Array.isArray(payload.layers) ? payload.layers : [];

  // Top file paths (capped to keep prompt small — the LLM only needs
  // enough to infer the tech stack and naming conventions)
  const file_paths: string[] = Array.isArray(payload.file_paths)
    ? payload.file_paths.slice(0, 300)
    : [];

  const layersStr = layers.map(l =>
    `  - id=${l.id} type=${l.layer_type} files=${l.file_count} ` +
    `dirs=[${l.dirs.join(", ")}] samples=[${l.sample_files.slice(0, 3).join(", ")}]`
  ).join("\n");

  const filesStr = file_paths.slice(0, 200).join("\n");

  const prompt = `You are a senior software architect. Analyze the repository ${owner}/${repo}.

Repository metadata:
  Language: ${primary_language}
  Description: ${description}
  Total files: ${total_files}

Heuristic architectural layers (from static file-tree clustering):
${layersStr}

Sample file paths from the repository:
${filesStr}

For each layer, write a SHORT repo-specific label (max 60 chars) that names the
actual technology or framework used, not a generic category. For example:
  - "Next.js App Router (React 19)" instead of "Client / Presentation Layer"
  - "tRPC API Routes + Edge Middleware" instead of "Server / API Services"
  - "Prisma ORM + Postgres Migrations" instead of "Data & State Management"

Also write a 1-2 sentence plain-English summary of what this repository IS and
does (not just its structure).

Return STRICT JSON only, no markdown fences:
{
  "summary": "<1-2 sentence repo-specific summary>",
  "layers": [
    { "id": "<same id as input>", "label": "<repo-specific label>", "tech_stack": "<specific tech>" }
  ]
}`;

  try {
    // ── Try free external providers first (Gemini → Groq) to save Cloudflare neurons ──
    // Gemini Flash Lite: 1,500 req/day free, 1M context — perfect for this task
    const geminiResp = await callGemini(prompt, env, "You are a senior software architect. Return STRICT JSON only.", 500);
    if (geminiResp) {
      const jsonMatch = geminiResp.text.match(/\{[\s\S]*\}/);
      if (jsonMatch) {
        console.log(`[architect] Phase 1 enrichment via ${geminiResp.provider} (free, 0 neurons)`);
        return jsonMatch[0];
      }
    }

    // Groq fallback (14,400 req/day free)
    const groqResp = await callGroq(prompt, env, "You are a senior software architect. Return STRICT JSON only.", 500);
    if (groqResp) {
      const jsonMatch = groqResp.text.match(/\{[\s\S]*\}/);
      if (jsonMatch) {
        console.log(`[architect] Phase 1 enrichment via ${groqResp.provider} (free, 0 neurons)`);
        return jsonMatch[0];
      }
    }

    // Last resort: Cloudflare Workers AI (costs neurons)
    console.log(`[architect] Phase 1 enrichment falling back to Cloudflare (external providers unavailable)`);
    const response = await env.AI.run(SUMMARY_MODEL as any, {
      messages: [{ role: "user", content: prompt }],
      max_tokens: 500,
    });
    const text = extractText(response) || "";
    // Extract JSON from the response (handle markdown fences if present)
    const jsonMatch = text.match(/\{[\s\S]*\}/);
    if (jsonMatch) {
      return jsonMatch[0];
    }
    // Fallback: return the raw text — the Rust side will handle gracefully
    return JSON.stringify({ summary: text.slice(0, 200), layers: [] });
  } catch {
    return JSON.stringify({ summary: "", layers: [] });
  }
}

// ---- Architecture Mapper: LLM Impact Narration ----
// Called by the architect sidebar to get an LLM explanation of a reverse BFS
// impact result. The graph algorithm discovers affected files + paths; the
// LLM narrates WHY each path matters in plain English.

async function handleImpactNarration(req: NexusRequest, env: Env): Promise<string> {
  const payload = req.task as any;
  const target_file: string = payload.target_file || "unknown";
  const affected_files: string[] = Array.isArray(payload.affected_files) ? payload.affected_files : [];
  const dependency_paths: string[][] = Array.isArray(payload.dependency_paths)
    ? payload.dependency_paths.map((p: any) => Array.isArray(p) ? p.map(String) : [String(p)])
    : [];
  const direct_count: number = payload.direct_count || 0;
  const transitive_count: number = payload.transitive_count || 0;
  const test_files: string[] = Array.isArray(payload.test_files) ? payload.test_files : [];
  const repo: string = payload.repo || "unknown";

  const pathsStr = dependency_paths.slice(0, 5).map((p) => p.join(" → ")).join("\n  ");
  const affectedStr = affected_files.slice(0, 10).join(", ");

  const prompt = `You are a senior software architect analyzing the impact of changing a file in the ${repo} repository.

The static dependency graph analysis found:
- Target file: ${target_file}
- Direct dependents (depth 1): ${direct_count}
- Transitive dependents (depth 2+): ${transitive_count}
- Test files affected: ${test_files.length}
- Critical dependency paths (target → root):
  ${pathsStr}
- Affected files: ${affectedStr}

Explain in plain English (under 150 words) what the developer should be careful about.
Focus on PRODUCTION RISK, not file count. Be specific about the most dangerous path.
If there are test files, note whether they provide adequate coverage.
Do NOT list every file — focus on the highest-risk path and why it matters.`;

  // Try free external providers first (Gemini → Groq → Cloudflare cascade)
  const synth = await synthesizeWithCascade(prompt, env,
    "You are a senior software architect. Explain production risk concisely.", 300);
  if (synth) {
    console.log(`[architect] Impact narration via ${synth.provider} (${synth.model})`);
    return synth.text;
  }
  // Last resort: Cloudflare summarize()
  return await summarize(prompt, env);
}

