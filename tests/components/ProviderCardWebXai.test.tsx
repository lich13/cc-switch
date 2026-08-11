import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderCard } from "@/components/providers/ProviderCard";
import type { Provider } from "@/types";

vi.mock("@/lib/runtime", () => ({ isWebRuntime: () => true }));

vi.mock("@/lib/query/failover", () => ({
  useProviderHealth: () => ({ data: null }),
}));

vi.mock("@/lib/query/queries", () => ({
  useUsageQuery: () => ({ data: null }),
}));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => <span data-testid="provider-icon" />,
}));

vi.mock("@/components/UsageFooter", () => ({ default: () => null }));
vi.mock("@/components/SubscriptionQuotaFooter", () => ({
  default: () => null,
}));
vi.mock("@/components/CopilotQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/CodexOauthQuotaFooter", () => ({ default: () => null }));
vi.mock("@/components/XaiOauthQuotaFooter", () => ({
  default: () => <span>xai-quota</span>,
}));

vi.mock("@/components/providers/ProviderActions", () => ({
  ProviderActions: ({ onEdit, onDuplicate, onDelete }: any) => (
    <div>
      {onEdit ? <button onClick={onEdit}>edit-entry</button> : null}
      {onDuplicate ? (
        <button onClick={onDuplicate}>duplicate-entry</button>
      ) : null}
      {onDelete ? <button onClick={onDelete}>delete-entry</button> : null}
    </div>
  ),
}));

const baseProvider: Provider = {
  id: "xai-provider",
  name: "xAI Provider",
  category: "custom",
  settingsConfig: {
    auth: { OPENAI_API_KEY: "secret_configured" },
    config:
      '[model_providers.xai]\nbase_url = "https://api.x.ai/v1"\nwire_api = "responses"',
  },
  meta: { apiFormat: "openai_responses" },
};

const renderProvider = (provider: Provider) =>
  render(
    <ProviderCard
      provider={provider}
      isCurrent={false}
      appId="codex"
      onSwitch={vi.fn()}
      onEdit={vi.fn()}
      onDelete={vi.fn()}
      onConfigureUsage={vi.fn()}
      onOpenWebsite={vi.fn()}
      onDuplicate={vi.fn()}
      isProxyRunning={false}
    />,
  );

describe("ProviderCard xAI WebUI actions", () => {
  it("hides all management and quota entry points for managed xAI OAuth data", () => {
    renderProvider({
      ...baseProvider,
      meta: {
        apiFormat: "openai_responses",
        providerType: "xai_oauth",
      },
    });

    expect(screen.getByText("xAI Provider")).toBeInTheDocument();
    expect(screen.queryByText("edit-entry")).not.toBeInTheDocument();
    expect(screen.queryByText("duplicate-entry")).not.toBeInTheDocument();
    expect(screen.queryByText("delete-entry")).not.toBeInTheDocument();
    expect(screen.queryByText("xai-quota")).not.toBeInTheDocument();
  });

  it("keeps normal xAI API-key providers editable in WebUI", () => {
    renderProvider(baseProvider);

    expect(screen.getByText("edit-entry")).toBeInTheDocument();
    expect(screen.getByText("duplicate-entry")).toBeInTheDocument();
  });
});
