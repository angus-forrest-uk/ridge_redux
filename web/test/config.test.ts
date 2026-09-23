// Exporting the current view to text, and importing one back.
import { describe, expect, test } from "vitest";
import { configToText, DEFAULTS, parseConfig } from "../src/lib/params.ts";

describe("config export/import", () => {
  test("round-trips the params", () => {
    expect(parseConfig(configToText(DEFAULTS))).toEqual(DEFAULTS);
  });

  test("accepts a bare hash and a permalink URL", () => {
    const hash = "#" + encodeURIComponent(JSON.stringify({ num_lines: 60, label: "Imported" }));
    expect(parseConfig(hash)).toEqual({ num_lines: 60, label: "Imported" });
    expect(parseConfig(`http://localhost:8420/${hash}`)).toEqual({ num_lines: 60, label: "Imported" });
  });

  test("rejects junk and a malformed bbox", () => {
    expect(parseConfig("")).toBeNull();
    expect(parseConfig("   ")).toBeNull();
    expect(parseConfig("not a config")).toBeNull();
    expect(parseConfig('{"bbox": [1, 2, 3]}')).toBeNull();
    expect(parseConfig('{"bbox": [1, 2, 3, null]}')).toBeNull();
    // A config without a bbox is fine: it keeps the current/default one.
    expect(parseConfig('{"num_lines": 60}')).toEqual({ num_lines: 60 });
  });
});
