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

  it("defaults omitted disableImageGeneration to chat", () => {
    expect(settingsSchema.parse(baseSettings).disableImageGeneration).toBe(
      "chat",
    );
  });

  it("defaults null and unknown disableImageGeneration to chat", () => {
    expect(
      settingsSchema.parse({
        ...baseSettings,
        disableImageGeneration: null,
      }).disableImageGeneration,
    ).toBe("chat");
    expect(
      settingsSchema.parse({
        ...baseSettings,
        disableImageGeneration: "__invalid__",
      }).disableImageGeneration,
    ).toBe("chat");
  });
});
