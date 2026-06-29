import { describe, expect, it } from "vitest";
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
});
