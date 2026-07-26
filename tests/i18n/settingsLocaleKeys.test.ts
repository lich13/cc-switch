import { describe, expect, it } from "vitest";
import en from "@/i18n/locales/en.json";
import ja from "@/i18n/locales/ja.json";
import zh from "@/i18n/locales/zh.json";
import zhTW from "@/i18n/locales/zh-TW.json";

const requiredSettingsKeys = [
  "manualInstallCommands",
  "updateAllTools",
  "currentVersion",
  "toolActionDone",
  "toolActionFailed",
  "toolActionInstalledNotRunnable",
  "toolActionPartial",
  "toolActionVersionUnchanged",
  "toolActionVersionUnchangedTitle",
  "toolCheckEnv",
  "toolConflictDefault",
  "toolConflictHint",
  "toolConflictNotRunnable",
  "toolConflictTitle",
  "toolDiagnose",
  "toolDiagnoseFailed",
  "toolDiagnoseNoConflict",
  "toolDiagnosing",
  "toolInstall",
  "toolNotRunnable",
  "toolReady",
  "toolUpdate",
  "toolUpgradeConfirmBtn",
  "toolUpgradeConfirmHint",
  "toolUpgradeConfirmTitle",
  "toolUpgradeUnanchoredHint",
  "toolUpgradeWillRun",
] as const;

describe("settings locale keys", () => {
  it("zh-TW includes tool install and upgrade copy", () => {
    for (const key of requiredSettingsKeys) {
      expect(zhTW.settings[key], key).toEqual(expect.any(String));
      expect(zhTW.settings[key].trim(), key).not.toBe("");
    }
  });

  it("removes the fork-only Claude User-Agent target copy from every locale", () => {
    for (const locale of [en, ja, zh, zhTW]) {
      const rewrite = locale.settings.advanced.userAgentRewrite as Record<
        string,
        unknown
      >;
      expect(rewrite).not.toHaveProperty("claudeTarget");
      expect(rewrite).not.toHaveProperty("claudeTargetRequired");
      expect(rewrite).toHaveProperty("codexTarget");
    }
  });
});
