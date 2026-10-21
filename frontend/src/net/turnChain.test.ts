import { beforeEach, describe, expect, it } from "vitest";
import { __testResetTurnChain, runInTurnOrder } from "./turnChain";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

describe("ghost turn order", () => {
  beforeEach(() => __testResetTurnChain());

  it("a slow older turn finishes before a fast newer one starts", async () => {
    const log: string[] = [];
    const slow = runInTurnOrder(async () => {
      log.push("A start");
      await sleep(40);
      log.push("A end");
    });
    const fast = runInTurnOrder(async () => {
      log.push("B start");
    });
    await Promise.all([slow, fast]);
    expect(log).toEqual(["A start", "A end", "B start"]);
  });

  it("a failing turn does not block the next one", async () => {
    const a = runInTurnOrder(async () => {
      throw new Error("boom");
    });
    await expect(a).rejects.toThrow("boom");
    const b = await runInTurnOrder(async () => "ok");
    expect(b).toBe("ok");
  });

  it("a stuck turn delays the next by at most maxWaitMs", async () => {
    runInTurnOrder(() => new Promise<void>(() => {}), 10); // never resolves
    const t0 = Date.now();
    const r = await runInTurnOrder(async () => "next", 30);
    expect(r).toBe("next");
    expect(Date.now() - t0).toBeLessThan(400);
  });
});
