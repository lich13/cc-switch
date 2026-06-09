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

  it("defaults omitted disableImageGeneration to false", () => {
    expect(settingsSchema.parse(baseSettings).disableImageGeneration).toBe(
      false,
    );
  });

  it("defaults null and unknown disableImageGeneration to false", () => {
    expect(
      settingsSchema.parse({
        ...baseSettings,
        disableImageGeneration: null,
      }).disableImageGeneration,
    ).toBe(false);
    expect(
      settingsSchema.parse({
        ...baseSettings,
        disableImageGeneration: "__invalid__",
      }).disableImageGeneration,
    ).toBe(false);
  });
});
