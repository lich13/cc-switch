import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UsageTrendChart } from "@/components/usage/UsageTrendChart";
import { useUsageTrends } from "@/lib/query/usage";

vi.mock("@/lib/query/usage", () => ({
  useUsageTrends: vi.fn(),
}));

vi.mock("recharts", () => ({
  Area: (props: any) => <div data-testid={`area-${props.dataKey}`} />,
  AreaChart: ({ children }: any) => (
    <svg data-testid="area-chart">{children}</svg>
  ),
  CartesianGrid: () => <div data-testid="cartesian-grid" />,
  Legend: () => <div data-testid="legend" />,
  ResponsiveContainer: ({ children }: any) => (
    <div data-testid="responsive-container">{children}</div>
  ),
  Tooltip: () => <div data-testid="tooltip" />,
  XAxis: () => <div data-testid="x-axis" />,
  YAxis: () => <div data-testid="y-axis" />,
}));

const props = {
  range: { preset: "today" as const },
  rangeLabel: "Today",
  refreshIntervalMs: 30000,
};

describe("UsageTrendChart", () => {
  beforeEach(() => {
    vi.mocked(useUsageTrends).mockReset();
  });

  it("keeps the real loading spinner while trend data is loading", () => {
    vi.mocked(useUsageTrends).mockReturnValue({
      data: undefined,
      isLoading: true,
    } as any);

    render(<UsageTrendChart {...props} />);

    expect(
      screen.queryByTestId("responsive-container"),
    ).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toBeInTheDocument();
  });

  it("renders after data returns even when layout measurement stays at zero", () => {
    vi.mocked(useUsageTrends).mockReturnValue({
      data: [
        {
          date: "2026-06-16T10:00:00Z",
          totalInputTokens: 10,
          totalOutputTokens: 20,
          totalCacheCreationTokens: 0,
          totalCacheReadTokens: 0,
          totalCost: 0.001,
        },
      ],
      isLoading: false,
    } as any);

    render(<UsageTrendChart {...props} />);

    expect(screen.getByTestId("responsive-container")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("shows an empty state after an empty trend response even without layout", () => {
    vi.mocked(useUsageTrends).mockReturnValue({
      data: [],
      isLoading: false,
    } as any);

    render(<UsageTrendChart {...props} />);

    expect(screen.getByText("usage.noTrendData")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
});
