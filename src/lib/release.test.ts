import { describe, expect, it } from "vitest";
import { releaseTagFromVersion } from "./release";

describe("releaseTagFromVersion", () => {
  it("maps fork internal versions to fork release tags", () => {
    expect(releaseTagFromVersion("3.16.0-10")).toBe("v3.16.0-lich13.10");
  });

  it("keeps explicit release tags unchanged", () => {
    expect(releaseTagFromVersion("v3.16.0-lich13.10")).toBe(
      "v3.16.0-lich13.10",
    );
  });

  it("keeps plain upstream versions as upstream-style tags", () => {
    expect(releaseTagFromVersion("3.16.0")).toBe("v3.16.0");
  });

  it("returns an empty tag for empty input", () => {
    expect(releaseTagFromVersion("  ")).toBe("");
  });
});
