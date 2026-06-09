import { describe, expect, it } from "vitest";
import { settingsSchema } from "./settings";

const baseSettings = {
  showInTray: true,
  minimizeToTrayOnClose: true,
};

describe("settingsSchema", () => {
  it("accepts the disableImageGeneration tri-state shape", () => {
    for (const disableImageGeneration of [false, true, "chat"] as const) {
      expect(
        settingsSchema.parse({
          ...baseSettings,
          disableImageGeneration,
        }).disableImageGeneration,
      ).toBe(disableImageGeneration);
    }
  });

  it("allows disableImageGeneration to be omitted", () => {
    expect(
      settingsSchema.parse(baseSettings).disableImageGeneration,
    ).toBeUndefined();
  });
});
