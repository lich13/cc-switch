import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { Provider } from "@/types";
import type { AppId } from "@/lib/api";
import { createTestQueryClient } from "../utils/testQueryClient";

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
  const queryClient = createTestQueryClient();
  render(
    <QueryClientProvider client={queryClient}>
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
      />
    </QueryClientProvider>,
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

  it.each(["claude", "claude-desktop", "gemini"] as AppId[])(
    "does not render the Codex-only policy in %s",
    (appId) => {
      renderCard(
        {
          ...baseProvider,
          meta: {
            apiFormat: "openai_responses",
            claudeDesktopMode: "proxy",
          },
        },
        vi.fn(),
        appId,
      );

      expect(
        screen.queryByRole("switch", {
          name: "provider.disableImageGenerationInChat",
        }),
      ).not.toBeInTheDocument();
    },
  );

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
