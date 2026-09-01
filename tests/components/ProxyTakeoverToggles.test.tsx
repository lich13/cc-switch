import type { ReactNode } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import "@testing-library/jest-dom";
import { QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ProxyPanel } from "@/components/proxy/ProxyPanel";
import { ProxyToggle } from "@/components/proxy/ProxyToggle";
import { getAppLabel } from "@/config/appConfig";
import { createTestQueryClient } from "../utils/testQueryClient";

const { invokeMock, toastSuccessMock, toastErrorMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  toastErrorMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      typeof options?.defaultValue === "string" ? options.defaultValue : key,
  }),
}));

vi.mock("framer-motion", () => ({
  AnimatePresence: ({ children }: { children: ReactNode }) => <>{children}</>,
  motion: {
    div: ({ children, ...props }: any) => <div {...props}>{children}</div>,
  },
}));

vi.mock("@/lib/query/failover", () => ({
  useFailoverQueue: () => ({ data: [] }),
  useProviderHealth: () => ({ data: undefined }),
}));

type RoutedApp = "claude" | "codex" | "gemini";
type TestRuntimeGlobal = typeof globalThis & {
  __CC_SWITCH_TEST_INVOKE__?: (
    command: string,
    payload?: { appType?: RoutedApp; enabled?: boolean },
  ) => Promise<unknown> | unknown;
};

const proxyStatus = {
  running: true,
  address: "127.0.0.1",
  port: 15721,
  active_connections: 0,
  total_requests: 0,
  success_requests: 0,
  failed_requests: 0,
  success_rate: 100,
  uptime_seconds: 1,
  current_provider: null,
  current_provider_id: null,
  last_request_at: null,
  last_error: null,
  failover_count: 0,
  active_targets: [],
};

function renderWithQueryClient(ui: ReactNode) {
  const queryClient = createTestQueryClient();
  render(<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>);
  return queryClient;
}

function setupInvokeMock({
  failStatusRefreshAfterTakeover = false,
  initialTakeoverStatus,
  failTakeoverForApp,
}: {
  failStatusRefreshAfterTakeover?: boolean;
  initialTakeoverStatus?: Partial<Record<RoutedApp, boolean>>;
  failTakeoverForApp?: RoutedApp;
} = {}) {
  (globalThis as TestRuntimeGlobal).__CC_SWITCH_TEST_INVOKE__ = (
    command,
    payload,
  ) => invokeMock(command, payload);

  const takeoverStatus: Record<RoutedApp, boolean> = {
    claude: false,
    codex: false,
    gemini: false,
    ...initialTakeoverStatus,
  };
  let takeoverMutationSucceeded = false;
  let statusRefreshFailures = 0;

  invokeMock.mockImplementation(
    (command: string, payload?: { appType?: RoutedApp; enabled?: boolean }) => {
      if (
        failStatusRefreshAfterTakeover &&
        takeoverMutationSucceeded &&
        (command === "get_proxy_status" ||
          command === "get_proxy_takeover_status")
      ) {
        statusRefreshFailures += 1;
        return Promise.reject(
          new Error("route status failure: codex.live.write_disabled"),
        );
      }

      if (command === "get_proxy_status") {
        return Promise.resolve(proxyStatus);
      }

      if (command === "get_proxy_takeover_status") {
        return Promise.resolve({
          ...takeoverStatus,
          opencode: false,
          openclaw: false,
          hermes: false,
        });
      }

      if (command === "set_proxy_takeover_for_app") {
        if (payload?.appType === failTakeoverForApp) {
          return Promise.reject(
            new Error("backend failure: codex.live.write_disabled"),
          );
        }
        if (payload?.appType) {
          takeoverStatus[payload.appType] = payload.enabled ?? false;
        }
        takeoverMutationSucceeded = true;
        return Promise.resolve(undefined);
      }

      if (command === "get_global_proxy_config") {
        return Promise.resolve({
          proxyEnabled: true,
          listenAddress: "127.0.0.1",
          listenPort: 15721,
          enableLogging: true,
        });
      }

      return Promise.resolve(undefined);
    },
  );

  return {
    getStatusRefreshFailures: () => statusRefreshFailures,
  };
}

function expectNoRouteStatusErrorToast() {
  expect(toastErrorMock).not.toHaveBeenCalled();

  const toastPayload = JSON.stringify([
    toastSuccessMock.mock.calls,
    toastErrorMock.mock.calls,
  ]);
  expect(toastPayload).not.toContain("codex.live.write_disabled");
  expect(toastPayload).not.toContain("route status failure");
  expect(toastPayload).not.toContain("切换路由状态失败");
}

function expectCommandCalled(command: string) {
  expect(invokeMock.mock.calls.some(([called]) => called === command)).toBe(
    true,
  );
}

function getPanelAppSwitch(appType: RoutedApp) {
  const row = screen.getByText(getAppLabel(appType)).closest("div");
  expect(row).not.toBeNull();
  return within(row as HTMLElement).getByRole("switch");
}

describe("Proxy takeover toggles", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
  });

  afterEach(() => {
    delete (globalThis as TestRuntimeGlobal).__CC_SWITCH_TEST_INVOKE__;
  });

  it("lets ProxyToggle enable Codex local routing when the backend succeeds without surfacing route-status refresh errors", async () => {
    const { getStatusRefreshFailures } = setupInvokeMock({
      failStatusRefreshAfterTakeover: true,
    });

    renderWithQueryClient(<ProxyToggle activeApp="codex" />);

    await waitFor(() => {
      expectCommandCalled("get_proxy_takeover_status");
    });

    fireEvent.click(screen.getByRole("switch"));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_proxy_takeover_for_app", {
        appType: "codex",
        enabled: true,
      });
    });
    await waitFor(() => {
      expect(getStatusRefreshFailures()).toBeGreaterThan(0);
    });

    expect(toastSuccessMock).toHaveBeenCalledWith(
      "已接管 Codex 配置（请求将走本地代理）",
      { closeButton: true },
    );
    expectNoRouteStatusErrorToast();
  });

  it("lets ProxyToggle disable Codex local routing when the backend succeeds without surfacing route-status errors", async () => {
    const { getStatusRefreshFailures } = setupInvokeMock({
      failStatusRefreshAfterTakeover: true,
      initialTakeoverStatus: { codex: true },
    });

    renderWithQueryClient(<ProxyToggle activeApp="codex" />);

    await waitFor(() => {
      expect(screen.getByRole("switch")).toBeChecked();
    });

    fireEvent.click(screen.getByRole("switch"));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_proxy_takeover_for_app", {
        appType: "codex",
        enabled: false,
      });
    });
    await waitFor(() => {
      expect(getStatusRefreshFailures()).toBeGreaterThan(0);
    });

    expect(toastSuccessMock).toHaveBeenCalledWith("已恢复 Codex 配置", {
      closeButton: true,
    });
    expectNoRouteStatusErrorToast();
  });

  it("keeps ProxyToggle backend failures visible", async () => {
    setupInvokeMock({ failTakeoverForApp: "codex" });

    renderWithQueryClient(<ProxyToggle activeApp="codex" />);

    await waitFor(() => {
      expectCommandCalled("get_proxy_takeover_status");
    });

    fireEvent.click(screen.getByRole("switch"));

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "操作失败: backend failure: codex.live.write_disabled",
      );
    });
  });

  it.each(["claude", "gemini"] as const)(
    "keeps ProxyToggle %s takeover behavior unchanged",
    async (appType) => {
      setupInvokeMock();

      renderWithQueryClient(<ProxyToggle activeApp={appType} />);

      await waitFor(() => {
        expectCommandCalled("get_proxy_takeover_status");
      });

      fireEvent.click(screen.getByRole("switch"));

      await waitFor(() => {
        expect(invokeMock).toHaveBeenCalledWith("set_proxy_takeover_for_app", {
          appType,
          enabled: true,
        });
      });
      expect(toastSuccessMock).toHaveBeenCalledTimes(1);
      expect(toastErrorMock).not.toHaveBeenCalled();
    },
  );

  it("lets ProxyPanel enable Codex local routing when the backend succeeds without surfacing route-status refresh errors", async () => {
    const { getStatusRefreshFailures } = setupInvokeMock({
      failStatusRefreshAfterTakeover: true,
    });

    renderWithQueryClient(
      <ProxyPanel
        enableLocalProxy
        onEnableLocalProxyChange={vi.fn()}
        onToggleProxy={vi.fn()}
        isProxyPending={false}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText(getAppLabel("codex"))).toBeInTheDocument();
    });

    fireEvent.click(getPanelAppSwitch("codex"));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_proxy_takeover_for_app", {
        appType: "codex",
        enabled: true,
      });
    });
    await waitFor(() => {
      expect(getStatusRefreshFailures()).toBeGreaterThan(0);
    });

    expect(toastSuccessMock).toHaveBeenCalledWith("codex 接管已启用", {
      closeButton: true,
    });
    expectNoRouteStatusErrorToast();
  });

  it("lets ProxyPanel disable Codex local routing when the backend succeeds without surfacing route-status refresh errors", async () => {
    const { getStatusRefreshFailures } = setupInvokeMock({
      failStatusRefreshAfterTakeover: true,
      initialTakeoverStatus: { codex: true },
    });

    renderWithQueryClient(
      <ProxyPanel
        enableLocalProxy
        onEnableLocalProxyChange={vi.fn()}
        onToggleProxy={vi.fn()}
        isProxyPending={false}
      />,
    );

    await waitFor(() => {
      expect(getPanelAppSwitch("codex")).toBeChecked();
    });

    fireEvent.click(getPanelAppSwitch("codex"));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("set_proxy_takeover_for_app", {
        appType: "codex",
        enabled: false,
      });
    });
    await waitFor(() => {
      expect(getStatusRefreshFailures()).toBeGreaterThan(0);
    });

    expect(toastSuccessMock).toHaveBeenCalledWith("codex 接管已关闭", {
      closeButton: true,
    });
    expectNoRouteStatusErrorToast();
  });

  it("keeps ProxyPanel backend failures visible", async () => {
    setupInvokeMock({ failTakeoverForApp: "codex" });

    renderWithQueryClient(
      <ProxyPanel
        enableLocalProxy
        onEnableLocalProxyChange={vi.fn()}
        onToggleProxy={vi.fn()}
        isProxyPending={false}
      />,
    );

    await waitFor(() => {
      expect(screen.getByText(getAppLabel("codex"))).toBeInTheDocument();
    });

    fireEvent.click(getPanelAppSwitch("codex"));

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        "切换接管状态失败",
      );
    });
  });

  it.each(["claude", "gemini"] as const)(
    "keeps ProxyPanel %s takeover behavior unchanged",
    async (appType) => {
      setupInvokeMock();

      renderWithQueryClient(
        <ProxyPanel
          enableLocalProxy
          onEnableLocalProxyChange={vi.fn()}
          onToggleProxy={vi.fn()}
          isProxyPending={false}
        />,
      );

      await waitFor(() => {
        expect(screen.getByText(getAppLabel(appType))).toBeInTheDocument();
      });

      fireEvent.click(getPanelAppSwitch(appType));

      await waitFor(() => {
        expect(invokeMock).toHaveBeenCalledWith("set_proxy_takeover_for_app", {
          appType,
          enabled: true,
        });
      });
      expect(toastSuccessMock).toHaveBeenCalledWith(`${appType} 接管已启用`, {
        closeButton: true,
      });
      expect(toastErrorMock).not.toHaveBeenCalled();
    },
  );
});
