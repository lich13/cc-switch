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

  it("defaults new installs to Codex and Grok Build only", () => {
    vi.mocked(isWebRuntime).mockReturnValue(false);

    render(<AppVisibilitySettings settings={settings} onChange={vi.fn()} />);

    expect(screen.getByRole("button", { name: "apps.codex" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(
      screen.getByRole("button", { name: "apps.grokbuild" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(
      screen.getByRole("button", { name: "apps.claudeCode" }),
    ).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "apps.gemini" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("lets WebUI expose Claude, Codex, Gemini and Grok Build without overriding explicit visibility", () => {
    vi.mocked(isWebRuntime).mockReturnValue(true);
    render(
      <AppVisibilitySettings
        settings={{
          ...settings,
          visibleApps: {
            claude: true,
            "claude-desktop": false,
            codex: false,
            gemini: true,
            grokbuild: false,
            opencode: false,
            openclaw: false,
            hermes: false,
            pi: false,
          },
        }}
        onChange={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("button", { name: "apps.claudeCode" }),
    ).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "apps.codex" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(screen.getByRole("button", { name: "apps.gemini" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(
      screen.getByRole("button", { name: "apps.grokbuild" }),
    ).toHaveAttribute("aria-pressed", "false");
    expect(
      screen.queryByRole("button", { name: "apps.claudeDesktop" }),
    ).not.toBeInTheDocument();
  });
});
