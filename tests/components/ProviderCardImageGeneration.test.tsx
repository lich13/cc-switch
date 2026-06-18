import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";

vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: null }),
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: null }),
}));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => <span data-testid="provider-icon" />,
}));

vi.mock("@/components/UsageFooter", () => ({
  default: () => <span data-testid="usage-footer" />,
}));

vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: () => <span data-testid="subscription-footer" />,
}));

vi.mock("@/components/CopilotQuotaFooter", () => ({
  default: () => <span data-testid="copilot-footer" />,
}));

vi.mock("@/components/CodexOauthQuotaFooter", () => ({
  default: () => <span data-testid="codex-oauth-footer" />,
}));

vi.mock("@/components/providers/ProviderActions", () => ({
  ProviderActions: () => <span data-testid="provider-actions" />,
}));

const baseProvider: Provider = {
  id: "provider-1",
  name: "Test Provider",
  category: "custom",
  settingsConfig: {
    config:
      'model_provider = "custom"\n[model_providers.custom]\nbase_url = "https://api.example.com/v1"\nwire_api = "responses"\n',
  },
  meta: {
    apiFormat: "openai_responses",
  },
};

function renderCard(
  provider: Provider,
  onToggleImageGenerationPolicy = vi.fn(),
  appId: AppId = "codex",
) {
  render(
    <ProviderCard
      provider={provider}
      isCurrent={false}
      appId={appId}
      onSwitch={vi.fn()}
      onEdit={vi.fn()}
      onDelete={vi.fn()}
      onConfigureUsage={vi.fn()}
      onOpenWebsite={vi.fn()}
      onDuplicate={vi.fn()}
      isProxyRunning={true}
      onToggleImageGenerationPolicy={onToggleImageGenerationPolicy}
    />,
  );

  return { onToggleImageGenerationPolicy };
}

describe("ProviderCard image generation policy", () => {
  it("renders the provider-level switch for routable providers", () => {
    renderCard(baseProvider);

    expect(
      screen.getByRole("switch", {
        name: "provider.disableImageGenerationInChat",
      }),
    ).toHaveAttribute("aria-checked", "false");
  });

  it("does not render the switch for official providers", () => {
    renderCard({
      ...baseProvider,
      category: "official",
      meta: { apiFormat: "openai_responses" },
    });

    expect(
      screen.queryByRole("switch", {
        name: "provider.disableImageGenerationInChat",
      }),
    ).not.toBeInTheDocument();
  });

  it("does not render the switch for managed account providers", () => {
    renderCard({
      ...baseProvider,
      meta: {
        apiFormat: "openai_responses",
        providerType: "github_copilot",
      },
    });

    expect(
      screen.queryByRole("switch", {
        name: "provider.disableImageGenerationInChat",
      }),
    ).not.toBeInTheDocument();
  });

  it("uses settingsConfig api_format fallback for Claude routable providers", () => {
    renderCard(
      {
        ...baseProvider,
        settingsConfig: {
          env: {
            ANTHROPIC_AUTH_TOKEN: "sk-test",
            ANTHROPIC_BASE_URL: "https://api.example.com",
          },
          api_format: "openai_chat",
        },
        meta: {},
      },
      vi.fn(),
      "claude",
    );

    expect(
      screen.getByRole("switch", {
        name: "provider.disableImageGenerationInChat",
      }),
    ).toBeInTheDocument();
  });

  it("does not render for Claude Desktop direct mode providers", () => {
    renderCard(
      {
        ...baseProvider,
        settingsConfig: {
          env: {
            ANTHROPIC_AUTH_TOKEN: "sk-test",
            ANTHROPIC_BASE_URL: "https://api.example.com",
          },
        },
        meta: {
          apiFormat: "openai_responses",
          claudeDesktopMode: "direct",
        },
      },
      vi.fn(),
      "claude-desktop",
    );

    expect(
      screen.queryByRole("switch", {
        name: "provider.disableImageGenerationInChat",
      }),
    ).not.toBeInTheDocument();
  });

  it("renders for Claude Desktop proxy mode providers", () => {
    renderCard(
      {
        ...baseProvider,
        settingsConfig: {
          env: {
            ANTHROPIC_AUTH_TOKEN: "sk-test",
            ANTHROPIC_BASE_URL: "https://api.example.com",
          },
        },
        meta: {
          apiFormat: "openai_responses",
          claudeDesktopMode: "proxy",
        },
      },
      vi.fn(),
      "claude-desktop",
    );

    expect(
      screen.getByRole("switch", {
        name: "provider.disableImageGenerationInChat",
      }),
    ).toBeInTheDocument();
  });

  it("toggles provider meta policy without relying on global settings", () => {
    const { onToggleImageGenerationPolicy } = renderCard(baseProvider);

    fireEvent.click(
      screen.getByRole("switch", {
        name: "provider.disableImageGenerationInChat",
      }),
    );

    expect(onToggleImageGenerationPolicy).toHaveBeenCalledWith(
      baseProvider,
      true,
    );
  });

  it("treats true and chat provider meta values as enabled", () => {
    renderCard({
      ...baseProvider,
      meta: { apiFormat: "openai_responses", disableImageGeneration: "chat" },
    });

    expect(
      screen.getByRole("switch", {
        name: "provider.disableImageGenerationInChat",
      }),
    ).toHaveAttribute("aria-checked", "true");
  });
});
