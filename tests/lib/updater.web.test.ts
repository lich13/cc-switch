import { beforeEach, describe, expect, it, vi } from "vitest";
import { checkForUpdate, getCurrentVersion } from "@/lib/updater";
import { getVersion, isWebRuntime } from "@/lib/runtime";
import { check } from "@tauri-apps/plugin-updater";

vi.mock("@/lib/runtime", () => ({
  getVersion: vi.fn(),
  isWebRuntime: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(),
}));

describe("updater web runtime", () => {
  beforeEach(() => {
    vi.mocked(isWebRuntime).mockReturnValue(true);
    vi.mocked(getVersion).mockResolvedValue("3.16.3-1");
    vi.mocked(check).mockReset();
  });

  it("gets the current version through the runtime adapter", async () => {
    await expect(getCurrentVersion()).resolves.toBe("3.16.3-1");

    expect(getVersion).toHaveBeenCalledTimes(1);
  });

  it("does not load the desktop updater plugin in web runtime", async () => {
    vi.mocked(check).mockRejectedValue(new Error("tauri updater unavailable"));

    await expect(checkForUpdate({ timeout: 1234 })).resolves.toEqual({
      status: "up-to-date",
    });

    expect(check).not.toHaveBeenCalled();
  });
});
