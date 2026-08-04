import { describe, expect, test } from "bun:test";
import { previewFolderName } from "../../src/components/RecordingSettings";

// The preview must match the Rust naming rules in
// src-tauri/src/audio/meeting_folder.rs, otherwise the settings screen promises
// a folder name the backend will not produce.
describe("previewFolderName", () => {
  test("applies the prefix verbatim, without inserting a separator", () => {
    expect(previewFolderName("Work_", "Standup")).toBe("Work_Standup_2026-07-28_10-30");
    expect(previewFolderName("Work", "Standup")).toBe("WorkStandup_2026-07-28_10-30");
  });

  test("renders no prefix for null, empty, or whitespace", () => {
    const expected = "Standup_2026-07-28_10-30";
    expect(previewFolderName(null, "Standup")).toBe(expected);
    expect(previewFolderName("", "Standup")).toBe(expected);
    expect(previewFolderName("   ", "Standup")).toBe(expected);
  });

  test("replaces path separators so the preview stays a single folder", () => {
    expect(previewFolderName("a/b-", "Standup")).toBe("a_b-Standup_2026-07-28_10-30");
    expect(previewFolderName(null, "Q1: Review")).toBe("Q1_ Review_2026-07-28_10-30");
  });

  test("strips trailing dots and spaces that Windows would drop", () => {
    expect(previewFolderName(null, "Standup...")).toBe("Standup_2026-07-28_10-30");
  });

  test("uses a sample meeting name by default", () => {
    expect(previewFolderName("Work_")).toBe("Work_Team Standup_2026-07-28_10-30");
  });
});
