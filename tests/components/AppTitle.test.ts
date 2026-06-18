import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

describe("app shell title", () => {
  it("uses cc-switch as the browser title", () => {
    const html = readFileSync(resolve("src/index.html"), "utf8");
    const title = html.match(/<title>(.*?)<\/title>/)?.[1];

    expect(title).toBe("cc-switch");
  });
});
