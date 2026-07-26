import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useXaiOauth } from "@/components/providers/forms/hooks/useXaiOauth";

const authGetStatusMock = vi.hoisted(() => vi.fn());
const runtimeMocks = vi.hoisted(() => ({
  isWebRuntime: vi.fn(() => false),
}));

vi.mock("@/lib/runtime", () => ({
  isWebRuntime: () => runtimeMocks.isWebRuntime(),
}));

vi.mock("@/lib/api", () => ({
  authApi: {
    authGetStatus: (...args: unknown[]) => authGetStatusMock(...args),
    authStartLogin: vi.fn(),
    authPollForAccount: vi.fn(),
    authLogout: vi.fn(),
    authRemoveAccount: vi.fn(),
    authSetDefaultAccount: vi.fn(),
  },
  settingsApi: { openExternal: vi.fn() },
}));

vi.mock("@/lib/clipboard", () => ({ copyText: vi.fn() }));

const createWrapper = () => {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
};

describe("useXaiOauth", () => {
  beforeEach(() => {
    runtimeMocks.isWebRuntime.mockReset();
    runtimeMocks.isWebRuntime.mockReturnValue(false);
    authGetStatusMock.mockReset().mockResolvedValue({
      provider: "xai_oauth",
      authenticated: false,
      default_account_id: null,
      accounts: [],
    });
  });

  it("does not mount the managed-account status query in WebUI", async () => {
    runtimeMocks.isWebRuntime.mockReturnValue(true);
    renderHook(() => useXaiOauth(), { wrapper: createWrapper() });

    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(authGetStatusMock).not.toHaveBeenCalled();
  });

  it("keeps desktop/default status loading enabled", async () => {
    runtimeMocks.isWebRuntime.mockReturnValue(false);
    renderHook(() => useXaiOauth(), { wrapper: createWrapper() });

    await waitFor(() =>
      expect(authGetStatusMock).toHaveBeenCalledWith("xai_oauth"),
    );
  });
});
