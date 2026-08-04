import { describe, expect, test } from "bun:test";
import { appendSummaryBullets } from "../../src/hooks/useLiveSummary";

// The live summary accumulates: each tick's bullets are appended to what is
// already there, so earlier bullets must survive verbatim.
describe("appendSummaryBullets", () => {
  test("joins two bullet lists into one contiguous list", () => {
    expect(appendSummaryBullets("- First point", "- Second point")).toBe(
      "- First point\n- Second point"
    );
  });

  test("preserves earlier bullets exactly", () => {
    const existing = "- Kickoff at 10am\n- Alice owns the migration";
    const combined = appendSummaryBullets(existing, "- Agreed to ship Friday");

    expect(combined.startsWith(existing)).toBe(true);
    expect(combined.split("\n")).toHaveLength(3);
  });

  test("returns the addition when there is no existing summary", () => {
    expect(appendSummaryBullets("", "- First point")).toBe("- First point");
    expect(appendSummaryBullets("   ", "- First point")).toBe("- First point");
  });

  test("returns the existing summary unchanged when nothing was added", () => {
    const existing = "- Kickoff at 10am";
    expect(appendSummaryBullets(existing, "")).toBe(existing);
    expect(appendSummaryBullets(existing, "   \n  ")).toBe(existing);
  });

  test("is empty when both sides are empty", () => {
    expect(appendSummaryBullets("", "")).toBe("");
  });

  test("does not leave a blank line that would split the markdown list", () => {
    const combined = appendSummaryBullets("- First\n\n", "\n\n- Second");
    expect(combined).toBe("- First\n- Second");
  });

  test("accumulates across many ticks without rewriting", () => {
    const ticks = ["- One", "- Two", "- Three", "- Four"];
    const result = ticks.reduce((acc, next) => appendSummaryBullets(acc, next), "");

    expect(result).toBe("- One\n- Two\n- Three\n- Four");
  });

  test("keeps indented sub-bullets attached to their parent", () => {
    const existing = "- Parent\n  - Child";
    const combined = appendSummaryBullets(existing, "- Next parent\n  - Next child");

    expect(combined).toBe("- Parent\n  - Child\n- Next parent\n  - Next child");
  });
});
