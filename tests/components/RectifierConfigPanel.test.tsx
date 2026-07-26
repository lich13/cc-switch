import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { RectifierConfigPanel } from "@/components/settings/RectifierConfigPanel";
import { settingsApi } from "@/lib/api/settings";
import type { UserAgentRewriteConfig } from "@/lib/api/settings";

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock("@/lib/api/settings", () => ({
  settingsApi: {
    getRectifierConfig: vi.fn(),
    setRectifierConfig: vi.fn(),
    getOptimizerConfig: vi.fn(),
    setOptimizerConfig: vi.fn(),
    getUserAgentRewriteConfig: vi.fn(),
    setUserAgentRewriteConfig: vi.fn(),
  },
}));

const baseRewriteConfig: UserAgentRewriteConfig = {
  enabled: true,
  codexTarget: "codex-tui/test",
  rules: [{ enabled: true, pattern: "^OpenAI/Python .*$" }],
};

function mockPanelConfig(
  userAgentRewriteConfig: UserAgentRewriteConfig = baseRewriteConfig,
) {
  vi.mocked(settingsApi.getRectifierConfig).mockResolvedValue({
    enabled: true,
    requestThinkingSignature: true,
    requestThinkingBudget: true,
    requestMediaFallback: true,
    requestMediaHeuristic: true,
  });
  vi.mocked(settingsApi.getOptimizerConfig).mockResolvedValue({
    enabled: false,
    thinkingOptimizer: true,
    cacheInjection: true,
  });
  vi.mocked(settingsApi.getUserAgentRewriteConfig).mockResolvedValue(
    userAgentRewriteConfig,
  );
  vi.mocked(settingsApi.setUserAgentRewriteConfig).mockResolvedValue(true);
}

describe("RectifierConfigPanel User-Agent rewrite settings", () => {
  beforeEach(() => {
    vi.mocked(settingsApi.getRectifierConfig).mockReset();
    vi.mocked(settingsApi.setRectifierConfig).mockReset();
    vi.mocked(settingsApi.getOptimizerConfig).mockReset();
    vi.mocked(settingsApi.setOptimizerConfig).mockReset();
    vi.mocked(settingsApi.getUserAgentRewriteConfig).mockReset();
    vi.mocked(settingsApi.setUserAgentRewriteConfig).mockReset();
  });

  it("preserves an intentionally empty regex list when saving", async () => {
    mockPanelConfig({
      enabled: true,
      codexTarget: "codex-tui/empty",
      rules: [],
    });

    render(<RectifierConfigPanel />);

    fireEvent.click(
      await screen.findByRole("button", {
        name: "settings.advanced.userAgentRewrite.save",
      }),
    );

    await waitFor(() => {
      expect(settingsApi.setUserAgentRewriteConfig).toHaveBeenCalledWith({
        enabled: true,
        codexTarget: "codex-tui/empty",
        rules: [],
      });
    });
  });

  it("saves the editable Codex target and per-rule enabled state", async () => {
    mockPanelConfig({
      enabled: true,
      codexTarget: "codex-tui/old",
      rules: [
        { enabled: true, pattern: "^OpenAI/Python .*$" },
        { enabled: false, pattern: "^curl/.*$" },
      ],
    });

    render(<RectifierConfigPanel />);

    fireEvent.change(
      await screen.findByLabelText(
        "settings.advanced.userAgentRewrite.codexTarget",
      ),
      { target: { value: " codex-tui/new " } },
    );

    fireEvent.click(
      screen.getAllByLabelText(
        "settings.advanced.userAgentRewrite.ruleEnabled",
      )[0],
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.advanced.userAgentRewrite.addRule",
      }),
    );
    const patternInputs = screen.getAllByPlaceholderText(
      "settings.advanced.userAgentRewrite.rulesPlaceholder",
    );
    fireEvent.change(patternInputs[2], {
      target: { value: " ^Codex/.*$ " },
    });
    fireEvent.click(
      screen.getAllByRole("button", {
        name: "settings.advanced.userAgentRewrite.deleteRule",
      })[1],
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.advanced.userAgentRewrite.save",
      }),
    );

    await waitFor(() => {
      expect(settingsApi.setUserAgentRewriteConfig).toHaveBeenCalledWith({
        enabled: true,
        codexTarget: "codex-tui/new",
        rules: [
          { enabled: false, pattern: "^OpenAI/Python .*$" },
          { enabled: true, pattern: "^Codex/.*$" },
        ],
      });
    });
  });

  it("only persists enabled when toggled and keeps the staged target and rules for explicit save", async () => {
    mockPanelConfig({
      enabled: false,
      codexTarget: "codex-tui/saved",
      rules: [{ enabled: true, pattern: "^OpenAI/Python .*$" }],
    });

    render(<RectifierConfigPanel />);

    fireEvent.click(
      await screen.findByRole("switch", {
        name: "settings.advanced.userAgentRewrite.enabled",
      }),
    );

    await waitFor(() => {
      expect(settingsApi.setUserAgentRewriteConfig).toHaveBeenCalledWith({
        enabled: true,
        codexTarget: "codex-tui/saved",
        rules: [{ enabled: true, pattern: "^OpenAI/Python .*$" }],
      });
    });

    fireEvent.change(
      screen.getByLabelText("settings.advanced.userAgentRewrite.codexTarget"),
      { target: { value: " codex-tui/staged " } },
    );
    fireEvent.change(
      screen.getByPlaceholderText(
        "settings.advanced.userAgentRewrite.rulesPlaceholder",
      ),
      { target: { value: " ^Staged/.*$ " } },
    );

    vi.mocked(settingsApi.setUserAgentRewriteConfig).mockClear();

    fireEvent.click(
      screen.getByRole("switch", {
        name: "settings.advanced.userAgentRewrite.enabled",
      }),
    );

    await waitFor(() => {
      expect(settingsApi.setUserAgentRewriteConfig).toHaveBeenCalledWith({
        enabled: false,
        codexTarget: "codex-tui/saved",
        rules: [{ enabled: true, pattern: "^OpenAI/Python .*$" }],
      });
    });

    fireEvent.click(
      screen.getByRole("switch", {
        name: "settings.advanced.userAgentRewrite.enabled",
      }),
    );
    await waitFor(() => {
      expect(settingsApi.setUserAgentRewriteConfig).toHaveBeenLastCalledWith({
        enabled: true,
        codexTarget: "codex-tui/saved",
        rules: [{ enabled: true, pattern: "^OpenAI/Python .*$" }],
      });
    });

    vi.mocked(settingsApi.setUserAgentRewriteConfig).mockClear();

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.advanced.userAgentRewrite.save",
      }),
    );

    await waitFor(() => {
      expect(settingsApi.setUserAgentRewriteConfig).toHaveBeenCalledWith({
        enabled: true,
        codexTarget: "codex-tui/staged",
        rules: [{ enabled: true, pattern: "^Staged/.*$" }],
      });
    });
  });

  it("has no Claude target UI and blocks an empty Codex target or invalid regex", async () => {
    mockPanelConfig();

    render(<RectifierConfigPanel />);

    expect(
      screen.queryByLabelText(
        "settings.advanced.userAgentRewrite.claudeTarget",
      ),
    ).not.toBeInTheDocument();
    fireEvent.change(
      await screen.findByLabelText(
        "settings.advanced.userAgentRewrite.codexTarget",
      ),
      { target: { value: " " } },
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.advanced.userAgentRewrite.save",
      }),
    );

    expect(settingsApi.setUserAgentRewriteConfig).not.toHaveBeenCalled();
    expect(toast.error).toHaveBeenCalledWith(
      "settings.advanced.userAgentRewrite.codexTargetRequired",
    );

    fireEvent.change(
      screen.getByLabelText("settings.advanced.userAgentRewrite.codexTarget"),
      { target: { value: "codex-tui/test" } },
    );
    fireEvent.change(
      screen.getByPlaceholderText(
        "settings.advanced.userAgentRewrite.rulesPlaceholder",
      ),
      { target: { value: "(" } },
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.advanced.userAgentRewrite.save",
      }),
    );

    expect(settingsApi.setUserAgentRewriteConfig).not.toHaveBeenCalled();
    expect(toast.error).toHaveBeenCalledWith(
      "settings.advanced.userAgentRewrite.invalidRegex",
    );
  });

  it("does not add rules beyond the configured limit", async () => {
    mockPanelConfig({
      enabled: true,
      codexTarget: "codex-tui/full",
      rules: Array.from({ length: 32 }, (_, index) => ({
        enabled: true,
        pattern: `^Agent/${index}$`,
      })),
    });

    render(<RectifierConfigPanel />);

    const addButton = await screen.findByRole("button", {
      name: "settings.advanced.userAgentRewrite.addRule",
    });

    expect(addButton).toBeDisabled();
    expect(
      screen.getAllByPlaceholderText(
        "settings.advanced.userAgentRewrite.rulesPlaceholder",
      ),
    ).toHaveLength(32);
  });
});
