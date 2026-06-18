import type { PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useProvidersQuery, type ProvidersQueryData } from "@/lib/query/queries";
import { providersApi } from "@/lib/api";
import type { Provider } from "@/types";

vi.mock("@/lib/api", () => ({
  providersApi: {
    getAll: vi.fn(),
    getCurrent: vi.fn(),
  },
  settingsApi: {
    get: vi.fn(),
  },
  usageApi: {
    query: vi.fn(),
  },
  sessionsApi: {
    list: vi.fn(),
    getMessages: vi.fn(),
  },
}));

const existingProvider: Provider = {
  id: "claude-1",
  name: "Claude Existing",
  settingsConfig: {},
  category: "custom",
  sortIndex: 0,
  createdAt: 1,
};

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: PropsWithChildren) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("useProvidersQuery", () => {
  beforeEach(() => {
    vi.mocked(providersApi.getAll).mockReset();
    vi.mocked(providersApi.getCurrent).mockReset();
    vi.spyOn(console, "error").mockImplementation(() => undefined);
  });

  it("keeps the previous providers when get_providers fails during a refetch", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const previousData: ProvidersQueryData = {
      providers: { [existingProvider.id]: existingProvider },
      currentProviderId: existingProvider.id,
    };
    queryClient.setQueryData(["providers", "claude"], previousData);
    vi.mocked(providersApi.getAll).mockRejectedValue(new Error("boom"));
    vi.mocked(providersApi.getCurrent).mockResolvedValue(existingProvider.id);

    const { result } = renderHook(() => useProvidersQuery("claude"), {
      wrapper: createWrapper(queryClient),
    });

    await waitFor(() => {
      expect(providersApi.getAll).toHaveBeenCalledTimes(1);
      expect(result.current.fetchStatus).toBe("idle");
    });

    expect(result.current.isError).toBe(true);
    expect(result.current.error).toEqual(new Error("boom"));
    expect(result.current.data).toEqual(previousData);
    expect(queryClient.getQueryData(["providers", "claude"])).toEqual(
      previousData,
    );
  });

  it("returns an empty providers map when get_providers succeeds with a real empty list", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    vi.mocked(providersApi.getAll).mockResolvedValue({});
    vi.mocked(providersApi.getCurrent).mockResolvedValue("");

    const { result } = renderHook(() => useProvidersQuery("claude"), {
      wrapper: createWrapper(queryClient),
    });

    await waitFor(() => {
      expect(result.current.isSuccess).toBe(true);
    });

    expect(result.current.data).toEqual({
      providers: {},
      currentProviderId: "",
    });
  });
});
