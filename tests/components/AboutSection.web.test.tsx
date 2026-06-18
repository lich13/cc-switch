import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AboutSection } from "@/components/settings/AboutSection";
import { getVersion, isWebRuntime } from "@/lib/runtime";

vi.mock("@/lib/runtime", () => ({
  getVersion: vi.fn(),
  isWebRuntime: vi.fn(),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: vi.fn().mockResolvedValue("9.9.9-tauri"),
}));

vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({
    hasUpdate: false,
    updateInfo: null,
    checkUpdate: vi.fn(),
    resetDismiss: vi.fn(),
    isChecking: false,
  }),
}));

describe("AboutSection web runtime", () => {
  beforeEach(() => {
    vi.mocked(isWebRuntime).mockReturnValue(true);
    vi.mocked(getVersion).mockResolvedValue("3.16.3-web");
  });

  it("loads the displayed app version through the runtime adapter", async () => {
    render(<AboutSection isPortable={false} />);

    await waitFor(() => {
      expect(screen.getByText("v3.16.3-web")).toBeInTheDocument();
    });

    expect(getVersion).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("v未知")).not.toBeInTheDocument();
  });
});
