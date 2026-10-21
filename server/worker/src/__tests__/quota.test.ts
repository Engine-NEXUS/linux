/**
 * Tests for the quota module.
 */

import { LIMITS, checkQuota, incrementUsage, getUsage } from "../quota";

// Mock D1
function mockDB() {
  const data: Record<string, any> = {};
  return {
    prepare: (sql: string) => ({
      bind: (...args: any[]) => ({
        first: async () => {
          if (sql.includes("SELECT") && sql.includes("usage_log")) {
            const key = `${args[0]}:${args[1]}`;
            return data[key] || null;
          }
          if (sql.includes("COALESCE(SUM")) {
            let total = 0;
            for (const k in data) {
              if (k.endsWith(`:${args[0]}`)) total += data[k].ai_neurons || 0;
            }
            return { total };
          }
          return null;
        },
        all: async () => ({ results: [] }),
        run: async () => {
          if (sql.includes("INSERT OR REPLACE INTO usage_log")) {
            const key = `${args[0]}:${args[1]}`;
            data[key] = {
              requests: args[2], ai_neurons: args[3], d1_reads: args[4],
              d1_writes: args[5], search_calls: args[6], deep_calls: args[7],
            };
          }
          return {};
        },
      }),
    }),
  };
}

const mockEnv: any = { DB: mockDB() as any, AI: {} as any };

describe("checkQuota", () => {
  test("allows request under limit", async () => {
    const result = await checkQuota(mockEnv, "user1", false, false);
    expect(result.allowed).toBe(true);
    expect(result.reason).toBeUndefined();
  });

  test("rejects when request limit exceeded", async () => {
    // Simulate max requests
    for (let i = 0; i < LIMITS.requests_per_day; i++) {
      await incrementUsage(mockEnv, "user2", { requests: 1 });
    }
    const result = await checkQuota(mockEnv, "user2", false, false);
    expect(result.allowed).toBe(false);
    expect(result.reason).toContain("Daily request limit");
  });

  test("rejects deep analysis when deep_calls limit exceeded", async () => {
    await incrementUsage(mockEnv, "user3", { deep_calls: LIMITS.deep_calls_per_day });
    const result = await checkQuota(mockEnv, "user3", true, false);
    expect(result.allowed).toBe(false);
    expect(result.reason).toContain("deep analysis");
  });

  test("rejects search when search_calls limit exceeded", async () => {
    await incrementUsage(mockEnv, "user4", { search_calls: LIMITS.search_calls_per_day });
    const result = await checkQuota(mockEnv, "user4", false, true);
    expect(result.allowed).toBe(false);
    expect(result.reason).toContain("search");
  });

  // P3.2 quota-aware narration: every denial reason must be speakable —
  // human sentence, names the limit, gives the recovery (tomorrow /
  // midnight / rephrase). A silent or empty reason = silent denial.
  test("every denial reason is speakable guidance", async () => {
    const cases: Array<[string, any, string, string]> = [
      ["requests", { requests: LIMITS.requests_per_day }, "user-a", "limit"],
      ["deep", { deep_calls: LIMITS.deep_calls_per_day }, "user-b", "deep"],
      ["search", { search_calls: LIMITS.search_calls_per_day }, "user-c", "search"],
      ["neurons", { ai_neurons: LIMITS.ai_neurons_per_day }, "user-d", "limit"],
    ];
    for (const [label, seed, user, mustContain] of cases) {
      await incrementUsage(mockEnv, user, seed);
      const isDeep = label === "deep";
      const isSearch = label === "search";
      const result = await checkQuota(mockEnv, user, isDeep, isSearch);
      expect(result.allowed, `${label} must deny`).toBe(false);
      expect(result.reason, `${label} reason must exist`).toBeTruthy();
      expect(result.reason!.length, `${label} reason must be a sentence`).toBeGreaterThan(15);
      expect(result.reason!.toLowerCase(), `${label} reason must guide`).toContain(mustContain);
    }
  });
});

describe("incrementUsage", () => {
  test("accumulates usage correctly", async () => {
    await incrementUsage(mockEnv, "user5", { requests: 1, ai_neurons: 50 });
    await incrementUsage(mockEnv, "user5", { requests: 1, ai_neurons: 100 });
    const usage = await getUsage(mockEnv, "user5");
    expect(usage.requests).toBe(2);
    expect(usage.ai_neurons).toBe(150);
  });
});
