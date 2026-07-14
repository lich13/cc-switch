import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppVisibilitySettings } from "@/components/settings/AppVisibilitySettings";
import type { SettingsFormState } from "@/hooks/useSettings";
import { isWebRuntime } from "@/lib/runtime";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/lib/runtime", () => ({
  isWebRuntime: vi.fn(),
}));

const settings: SettingsFormState = {
  language: "en",
  showInTray: true,
  minimizeToTrayOnClose: true,
};

describe("AppVisibilitySettings web runtime", () => {
  beforeEach(() => {
    vi.mocked(isWebRuntime).mockReset();
  });

  it("does not show the profile switcher setting in web runtime", () => {
    vi.mocked(isWebRuntime).mockReturnValue(true);

    render(<AppVisibilitySettings settings={settings} onChange={vi.fn()} />);

    expect(
      screen.queryByText("settings.appVisibility.showProfileSwitcher"),
    ).not.toBeInTheDocument();
  });

  it("keeps the profile switcher setting in desktop runtime", () => {
    vi.mocked(isWebRuntime).mockReturnValue(false);

    render(<AppVisibilitySettings settings={settings} onChange={vi.fn()} />);

    expect(
      screen.getByText("settings.appVisibility.showProfileSwitcher"),
    ).toBeInTheDocument();
  });
});
