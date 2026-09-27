/**
 * Regression tests for GitHub PR routing regex patterns.
 * Covers Bug 2 (list PRs in owner/repo) and Bug 3 (check latest PR).
 *
 * These tests replicate the regex patterns from index.ts to verify
 * the parsing logic without needing a full Worker environment.
 */

describe("Bug 2: listPrMatch regex extracts repo from group 1", () => {
  // Replicated from index.ts handleGitHub
  const listPrMatch = (t: string) =>
    t.match(/(?:list|show|open)\s+(?:open\s+)?(?:prs|pull requests?)(?:\s+(?:in|of|from)\s+([\w\-./]+))?/);

  test("extracts owner/repo from 'list PRs in owner/repo'", () => {
    const m = listPrMatch("list prs in chitkullakshya/servx");
    expect(m).not.toBeNull();
    expect(m![1]).toBe("chitkullakshya/servx");
  });

  test("extracts owner/repo from 'show pull requests in owner/repo'", () => {
    const m = listPrMatch("show pull requests in chitkullakshya/servx");
    expect(m).not.toBeNull();
    expect(m![1]).toBe("chitkullakshya/servx");
  });

  test("extracts single-word repo from 'list open PRs from myrepo'", () => {
    const m = listPrMatch("list open prs from myrepo");
    expect(m).not.toBeNull();
    expect(m![1]).toBe("myrepo");
  });

  test("matches without a repo (group 1 is undefined)", () => {
    const m = listPrMatch("list prs");
    expect(m).not.toBeNull();
    expect(m![1]).toBeUndefined();
  });
});

describe("Bug 3: latestPrMatch regex catches latest/current PR phrases", () => {
  // Replicated from index.ts handleGitHub
  const latestPrMatch = (t: string) =>
    t.match(/(?:check|view|see|get|show|look\s+at|latest|current|newest|most\s+recent|recent|last)\s+(?:the\s+)?(?:latest\s+|current\s+|newest\s+|most\s+recent\s+)?(?:pr|pull\s*request)(?:\s+(?:in|of|from)\s+([\w\-./]+))?/);

  test("matches 'check latest PR'", () => {
    expect(latestPrMatch("check latest pr")).not.toBeNull();
  });

  test("matches 'check latest PR in owner/repo' and extracts repo", () => {
    const m = latestPrMatch("check latest pr in chitkullakshya/servx");
    expect(m).not.toBeNull();
    expect(m![1]).toBe("chitkullakshya/servx");
  });

  test("matches 'view current PR'", () => {
    expect(latestPrMatch("view current pr")).not.toBeNull();
  });

  test("matches 'see the latest pull request'", () => {
    expect(latestPrMatch("see the latest pull request")).not.toBeNull();
  });

  test("matches 'get newest PR in myrepo' and extracts repo", () => {
    const m = latestPrMatch("get newest pr in myrepo");
    expect(m).not.toBeNull();
    expect(m![1]).toBe("myrepo");
  });

  test("matches 'most recent PR'", () => {
    expect(latestPrMatch("most recent pr")).not.toBeNull();
  });

  test("matches 'last PR in owner/repo' and extracts repo", () => {
    const m = latestPrMatch("last pr in owner/repo");
    expect(m).not.toBeNull();
    expect(m![1]).toBe("owner/repo");
  });

  test("does NOT match 'list PRs' (that goes to listPrMatch)", () => {
    expect(latestPrMatch("list prs")).toBeNull();
  });

  test("does NOT match 'PR 24' (that goes to prMatch)", () => {
    expect(latestPrMatch("pr 24")).toBeNull();
  });
});

describe("Bug 3: keywordFallback routes 'check latest PR' to github", () => {
  // Replicated from index.ts keywordFallback — only the final github check
  // that 'check latest PR' should hit.
  const githubKeywordCheck = (t: string) =>
    /\b(pr|pull request|repo|repository|commit|issue|branch|merge|github|list\s+prs)\b/.test(t);

  test("'check latest PR' matches github keyword", () => {
    expect(githubKeywordCheck("check latest pr")).toBe(true);
  });

  test("'view current PR' matches github keyword", () => {
    expect(githubKeywordCheck("view current pr")).toBe(true);
  });
});
