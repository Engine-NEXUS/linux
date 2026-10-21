import { beforeEach, describe, expect, it } from "vitest";

import {
  __testResetMicHolders,
  micAcquire,
  micFree,
  micHoldersSummary,
  micLiveHolders,
  micRelease,
} from "./micHolders";

describe("micHolders (approach C audit)", () => {
  beforeEach(() => {
    __testResetMicHolders();
  });

  it("starts free with no holders", () => {
    expect(micFree()).toBe(true);
    expect(micLiveHolders()).toEqual([]);
    expect(micHoldersSummary()).toBe("holders=none(free)");
  });

  it("acquire holds, release frees", () => {
    const id = micAcquire("warm");
    expect(micFree()).toBe(false);
    expect(micLiveHolders()).toHaveLength(1);
    expect(micLiveHolders()[0].reason).toBe("warm");
    micRelease(id);
    expect(micFree()).toBe(true);
  });

  it("unknown release ids are ignored", () => {
    micRelease("nope#99");
    expect(micFree()).toBe(true);
  });

  it("double release is idempotent", () => {
    const id = micAcquire("vad");
    micRelease(id);
    micRelease(id);
    expect(micFree()).toBe(true);
  });

  it("summary names live holders with ages for debug_trace", () => {
    micAcquire("param-capture");
    const s = micHoldersSummary();
    expect(s).toContain("holders=param-capture@");
    expect(s).toContain("s");
  });

  it("multiple concurrent holders all list", () => {
    micAcquire("a");
    micAcquire("b");
    expect(micLiveHolders()).toHaveLength(2);
    expect(micFree()).toBe(false);
  });
});
