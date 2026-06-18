import { describe, expect, it } from "vitest";
import { settingsSchema } from "./settings";

const parseSettings = (...args: [unknown?]) => {
  const [disableImageGeneration] = args;
  return settingsSchema.parse({
    showInTray: true,
    minimizeToTrayOnClose: true,
    ...(args.length > 0 ? { disableImageGeneration } : {}),
  });
};

describe("settingsSchema", () => {
  it.each([
    [false, false],
    [true, true],
    ["chat", "chat"],
  ])("preserves disableImageGeneration value %s", (input, expected) => {
    expect(parseSettings(input).disableImageGeneration).toBe(expected);
  });

  it.each([undefined, null, "unknown", "all", 1, {}])(
    "normalizes unsupported disableImageGeneration value %s to false",
    (input) => {
      expect(parseSettings(input).disableImageGeneration).toBe(false);
    },
  );

  it("defaults omitted disableImageGeneration to false", () => {
    expect(parseSettings().disableImageGeneration).toBe(false);
  });
});
