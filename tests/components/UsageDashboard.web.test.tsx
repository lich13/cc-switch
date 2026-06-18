import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageDashboard } from "@/components/usage/UsageDashboard";
import { isWebRuntime } from "@/lib/runtime";

vi.mock("@/lib/runtime", () => ({
  isWebRuntime: vi.fn(),
}));

vi.mock("@/hooks/useUsageEventBridge", () => ({
  useUsageEventBridge: vi.fn(),
}));

vi.mock("@/lib/query/usage", () => ({
  usageKeys: { all: ["usage"] },
  useProviderStats: vi.fn(() => ({ data: [] })),
  useModelStats: vi.fn(() => ({ data: [] })),
}));

vi.mock("@/components/usage/UsageHero", () => ({
  UsageHero: () => <div data-testid="usage-hero" />,
}));

vi.mock("@/components/usage/UsageTrendChart", () => ({
  UsageTrendChart: () => <div data-testid="usage-trend-chart" />,
}));

vi.mock("@/components/usage/RequestLogTable", () => ({
  RequestLogTable: () => <div data-testid="request-log-table" />,
}));

vi.mock("@/components/usage/ProviderStatsTable", () => ({
  ProviderStatsTable: () => <div data-testid="provider-stats-table" />,
}));

vi.mock("@/components/usage/ModelStatsTable", () => ({
  ModelStatsTable: () => <div data-testid="model-stats-table" />,
}));

vi.mock("@/components/usage/PricingConfigPanel", () => ({
  PricingConfigPanel: () => <div data-testid="pricing-config-panel" />,
}));

function renderUsageDashboard() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <UsageDashboard />
    </QueryClientProvider>,
  );
}

describe("UsageDashboard web runtime", () => {
  beforeEach(() => {
    vi.mocked(isWebRuntime).mockReset();
  });

  it("does not render the usage trend chart in web runtime", () => {
    vi.mocked(isWebRuntime).mockReturnValue(true);

    renderUsageDashboard();

    expect(screen.getByTestId("usage-hero")).toBeInTheDocument();
    expect(screen.queryByTestId("usage-trend-chart")).not.toBeInTheDocument();
  });

  it("keeps the usage trend chart in desktop runtime", () => {
    vi.mocked(isWebRuntime).mockReturnValue(false);

    renderUsageDashboard();

    expect(screen.getByTestId("usage-trend-chart")).toBeInTheDocument();
  });
});
