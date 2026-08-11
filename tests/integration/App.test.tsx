import { Suspense, type ComponentType } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  render,
  screen,
  waitFor,
  fireEvent,
  within,
} from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { providersApi } from "@/lib/api/providers";
import {
  resetProviderState,
  setCurrentProviderId,
  setLiveProviderIds,
  setProviderForEdit,
  setProviders,
  setSettings,
} from "../msw/state";
import { server } from "../msw/server";
import { emitTauriEvent } from "../msw/tauriMocks";

vi.setConfig({ testTimeout: 20_000 });

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();
const runtimeMocks = vi.hoisted(() => ({
  isWebRuntime: vi.fn(() => false),
}));

const skillsPanelMocks = vi.hoisted(() => ({
  checkUpdates: vi.fn(),
  openDiscovery: vi.fn(),
}));

vi.mock("@/lib/runtime", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/runtime")>("@/lib/runtime");
  return {
    ...actual,
    isWebRuntime: () => runtimeMocks.isWebRuntime(),
  };
});

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

vi.mock("@/components/providers/ProviderList", () => ({
  ProviderList: ({
    providers,
    currentProviderId,
    onSwitch,
    onEdit,
    onDuplicate,
    onConfigureUsage,
    onOpenWebsite,
    onCreate,
  }: any) => (
    <div>
      <div data-testid="provider-list">{JSON.stringify(providers)}</div>
      <div data-testid="current-provider">{currentProviderId}</div>
      <button onClick={() => onSwitch(providers[currentProviderId])}>
        switch
      </button>
      <button onClick={() => onEdit(providers[currentProviderId])}>edit</button>
      <button onClick={() => onDuplicate(providers[currentProviderId])}>
        duplicate
      </button>
      <button onClick={() => onConfigureUsage(providers[currentProviderId])}>
        usage
      </button>
      <button onClick={() => onOpenWebsite("https://example.com")}>
        open-website
      </button>
      <button onClick={() => onCreate?.()}>create</button>
    </div>
  ),
}));

vi.mock("@/components/providers/AddProviderDialog", () => ({
  AddProviderDialog: ({ open, onOpenChange, onSubmit, appId }: any) =>
    open ? (
      <div data-testid="add-provider-dialog">
        <button
          onClick={() =>
            onSubmit({
              name: `New ${appId} Provider`,
              settingsConfig: {},
              category: "custom",
              sortIndex: 99,
            })
          }
        >
          confirm-add
        </button>
        <button onClick={() => onOpenChange(false)}>close-add</button>
      </div>
    ) : null,
}));

vi.mock("@/components/providers/EditProviderDialog", () => ({
  EditProviderDialog: ({
    open,
    provider,
    onSubmit,
    onOpenChange,
    onExitComplete,
  }: any) => (
    <div data-testid="edit-provider-shell" data-open={String(open)}>
      <output data-testid="edit-provider-payload">
        {provider ? JSON.stringify(provider) : ""}
      </output>
      {open ? (
        <div data-testid="edit-provider-dialog">
          <button
            onClick={async () => {
              await onSubmit({
                provider: {
                  ...provider,
                  name: `${provider.name}-edited`,
                },
                originalId: provider.id,
              });
              onOpenChange(false);
            }}
          >
            confirm-edit
          </button>
          <button onClick={() => onOpenChange(false)}>close-edit</button>
        </div>
      ) : null}
      <button onClick={() => onExitComplete?.()}>finish-edit-exit</button>
    </div>
  ),
}));

vi.mock("@/components/UsageScriptModal", () => ({
  default: ({ isOpen, provider, onSave, onClose }: any) =>
    isOpen ? (
      <div data-testid="usage-modal">
        <span data-testid="usage-provider">{provider?.id}</span>
        <button onClick={() => onSave("script-code")}>save-script</button>
        <button onClick={() => onClose()}>close-usage</button>
      </div>
    ) : null,
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: ({ isOpen, onConfirm, onCancel }: any) =>
    isOpen ? (
      <div data-testid="confirm-dialog">
        <button onClick={() => onConfirm()}>confirm-delete</button>
        <button onClick={() => onCancel()}>cancel-delete</button>
      </div>
    ) : null,
}));

vi.mock("@/components/AppSwitcher", () => ({
  AppSwitcher: ({ activeApp, onSwitch }: any) => (
    <div data-testid="app-switcher">
      <span>{activeApp}</span>
      <button onClick={() => onSwitch("claude")}>switch-claude</button>
      <button onClick={() => onSwitch("codex")}>switch-codex</button>
      <button onClick={() => onSwitch("openclaw")}>switch-openclaw</button>
    </div>
  ),
}));

vi.mock("@/components/skills/UnifiedSkillsPanel", async () => {
  const React = await import("react");
  const MockUnifiedSkillsPanel = React.forwardRef(
    ({ onCheckUpdatesStateChange }: any, ref) => {
      React.useEffect(() => {
        onCheckUpdatesStateChange?.({ isChecking: false, hasSkills: true });
        return () =>
          onCheckUpdatesStateChange?.({
            isChecking: false,
            hasSkills: false,
          });
      }, [onCheckUpdatesStateChange]);
      React.useImperativeHandle(ref, () => ({
        openDiscovery: skillsPanelMocks.openDiscovery,
        openImport: vi.fn(),
        openInstallFromZip: vi.fn(),
        openRestoreFromBackup: vi.fn(),
        checkUpdates: skillsPanelMocks.checkUpdates,
      }));
      return <div data-testid="unified-skills-panel" />;
    },
  );
  MockUnifiedSkillsPanel.displayName = "MockUnifiedSkillsPanel";
  return { default: MockUnifiedSkillsPanel };
});

vi.mock("@/components/UpdateBadge", () => ({
  UpdateBadge: ({ onClick }: any) => (
    <button onClick={onClick}>update-badge</button>
  ),
}));

vi.mock("@/components/mcp/McpPanel", () => ({
  default: ({ open, onOpenChange }: any) =>
    open ? (
      <div data-testid="mcp-panel">
        <button onClick={() => onOpenChange(false)}>close-mcp</button>
      </div>
    ) : (
      <button onClick={() => onOpenChange(true)}>open-mcp</button>
    ),
}));

const renderApp = (AppComponent: ComponentType) => {
  const client = new QueryClient();
  const result = render(
    <QueryClientProvider client={client}>
      <Suspense fallback={<div data-testid="loading">loading</div>}>
        <AppComponent />
      </Suspense>
    </QueryClientProvider>,
  );
  return { ...result, client };
};

describe("App integration with MSW", () => {
  beforeEach(() => {
    resetProviderState();
    runtimeMocks.isWebRuntime.mockReset();
    runtimeMocks.isWebRuntime.mockReturnValue(false);
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    skillsPanelMocks.checkUpdates.mockReset();
    skillsPanelMocks.openDiscovery.mockReset();
    window.localStorage.clear();
  });

  it("covers basic provider flows via real hooks", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );

    fireEvent.click(screen.getByText("usage"));
    expect(screen.getByTestId("usage-modal")).toBeInTheDocument();
    fireEvent.click(screen.getByText("save-script"));
    fireEvent.click(screen.getByText("close-usage"));

    fireEvent.click(screen.getByText("create"));
    expect(screen.getByTestId("add-provider-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByText("confirm-add"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(
        /New codex Provider/,
      ),
    );

    fireEvent.click(screen.getByText("edit"));
    expect(screen.getByTestId("edit-provider-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByText("confirm-edit"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(
        /-edited/,
      ),
    );

    fireEvent.click(screen.getByText("switch"));
    fireEvent.click(screen.getByText("duplicate"));
    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toMatch(/copy/),
    );

    fireEvent.click(screen.getByText("open-website"));

    emitTauriEvent("provider-switched", {
      appType: "codex",
      providerId: "codex-2",
    });

    expect(toastErrorMock).not.toHaveBeenCalled();
    expect(toastSuccessMock).toHaveBeenCalled();
  }, 10_000);

  it("shows toast when auto sync fails in background", async () => {
    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "codex-1",
      ),
    );

    expect(() => {
      emitTauriEvent("webdav-sync-status-updated", null);
    }).not.toThrow();
    expect(toastErrorMock).not.toHaveBeenCalled();

    emitTauriEvent("webdav-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "network timeout",
    });

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });

    toastErrorMock.mockReset();
    expect(() => {
      emitTauriEvent("s3-sync-status-updated", null);
    }).not.toThrow();
    expect(toastErrorMock).not.toHaveBeenCalled();

    emitTauriEvent("s3-sync-status-updated", {
      source: "auto",
      status: "error",
      error: "s3 timeout",
    });

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalled();
    });
  });

  it("duplicates openclaw providers with a generated key that avoids live-only ids", async () => {
    setSettings({
      visibleApps: {
        claude: false,
        "claude-desktop": false,
        codex: true,
        gemini: false,
        grokbuild: true,
        opencode: false,
        openclaw: true,
        hermes: false,
      },
    });
    setProviders("openclaw", {
      deepseek: {
        id: "deepseek",
        name: "DeepSeek",
        settingsConfig: {
          baseUrl: "https://api.deepseek.com",
          apiKey: "test-key",
          api: "openai-completions",
          models: [],
        },
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("openclaw", "deepseek");
    setLiveProviderIds("openclaw", ["deepseek-copy"]);

    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-openclaw"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "deepseek",
      ),
    );

    fireEvent.click(screen.getByText("duplicate"));

    await waitFor(() => {
      const providerList = screen.getByTestId("provider-list").textContent;
      expect(providerList).toContain("deepseek-copy-2");
      expect(providerList).toContain("DeepSeek copy");
    });

    expect(toastErrorMock).not.toHaveBeenCalledWith(
      expect.stringContaining("Provider key is required for openclaw"),
    );
  });

  it("shows toast when duplicate cannot load live provider ids", async () => {
    setSettings({
      visibleApps: {
        claude: false,
        "claude-desktop": false,
        codex: true,
        gemini: false,
        grokbuild: true,
        opencode: false,
        openclaw: true,
        hermes: false,
      },
    });
    setProviders("openclaw", {
      deepseek: {
        id: "deepseek",
        name: "DeepSeek",
        settingsConfig: {
          baseUrl: "https://api.deepseek.com",
          apiKey: "test-key",
          api: "openai-completions",
          models: [],
        },
        category: "custom",
        sortIndex: 0,
        createdAt: Date.now(),
      },
    });
    setCurrentProviderId("openclaw", "deepseek");

    const liveIdsSpy = vi
      .spyOn(providersApi, "getOpenClawLiveProviderIds")
      .mockRejectedValueOnce(new Error("broken config"));

    const { default: App } = await import("@/App");
    renderApp(App);

    fireEvent.click(screen.getByText("switch-openclaw"));

    await waitFor(() =>
      expect(screen.getByTestId("provider-list").textContent).toContain(
        "deepseek",
      ),
    );

    fireEvent.click(screen.getByText("duplicate"));

    await waitFor(() => {
      expect(toastErrorMock).toHaveBeenCalledWith(
        expect.stringContaining("读取配置中的供应商标识失败"),
      );
    });

    expect(screen.getByTestId("provider-list").textContent).not.toContain(
      "deepseek-copy",
    );

    liveIdsSpy.mockRestore();
  });

  it("loads a web provider edit detail on demand and clears it after exit", async () => {
    runtimeMocks.isWebRuntime.mockReturnValue(true);
    window.localStorage.setItem("cc-switch-last-app", "codex");

    const maskedProvider = {
      id: "codex-secret",
      name: "Masked Codex",
      category: "custom" as const,
      settingsConfig: {
        auth: { OPENAI_API_KEY: "secret_configured" },
        config: 'model = "gpt-5"',
      },
    };
    const editDetail = {
      ...maskedProvider,
      settingsConfig: {
        ...maskedProvider.settingsConfig,
        auth: { OPENAI_API_KEY: "sk-web-edit-detail" },
      },
    };
    setProviders("codex", { [maskedProvider.id]: maskedProvider });
    setProviderForEdit("codex", maskedProvider.id, editDetail);
    setCurrentProviderId("codex", maskedProvider.id);

    const { default: App } = await import("@/App");
    const { client } = renderApp(App);

    await waitFor(() => {
      expect(screen.getByTestId("provider-list")).toHaveTextContent(
        "secret_configured",
      );
    });
    expect(screen.getByTestId("provider-list")).not.toHaveTextContent(
      "sk-web-edit-detail",
    );

    fireEvent.click(screen.getByText("edit"));

    await waitFor(() => {
      expect(screen.getByTestId("edit-provider-dialog")).toBeInTheDocument();
      expect(screen.getByTestId("edit-provider-payload")).toHaveTextContent(
        "sk-web-edit-detail",
      );
    });

    const queryData = client
      .getQueryCache()
      .getAll()
      .map((query) => query.state.data);
    expect(JSON.stringify(queryData)).not.toContain("sk-web-edit-detail");
    expect(JSON.stringify(window.localStorage)).not.toContain(
      "sk-web-edit-detail",
    );

    fireEvent.click(screen.getByText("close-edit"));
    expect(
      screen.queryByTestId("edit-provider-dialog"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("edit-provider-payload")).toHaveTextContent(
      "sk-web-edit-detail",
    );

    fireEvent.click(screen.getByText("finish-edit-exit"));
    await waitFor(() => {
      expect(screen.getByTestId("edit-provider-payload")).toBeEmptyDOMElement();
    });
  });

  it("does not open the web edit panel when loading detail fails", async () => {
    runtimeMocks.isWebRuntime.mockReturnValue(true);
    window.localStorage.setItem("cc-switch-last-app", "codex");
    server.use(
      http.post("http://tauri.local/get_provider_for_edit", () =>
        HttpResponse.json({ error: "detail unavailable" }, { status: 500 }),
      ),
    );

    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list")).toHaveTextContent("codex-1"),
    );
    fireEvent.click(screen.getByText("edit"));

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    expect(
      screen.queryByTestId("edit-provider-dialog"),
    ).not.toBeInTheDocument();
  });

  it("keeps compact Web provider toolbar actions accessible", async () => {
    runtimeMocks.isWebRuntime.mockReturnValue(true);
    window.localStorage.setItem("cc-switch-last-app", "codex");

    const { default: App } = await import("@/App");
    renderApp(App);

    await waitFor(() =>
      expect(screen.getByTestId("provider-list")).toHaveTextContent("codex-1"),
    );
    fireEvent.click(
      screen.getByRole("button", { name: "firstRunNotice.confirm" }),
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "firstRunNotice.title" }),
      ).not.toBeInTheDocument(),
    );

    const exportButton = screen.getByRole("button", {
      name: "settings.exportProvidersSub2api",
    });
    expect(exportButton).toHaveClass("w-8", "sm:w-auto");
    expect(exportButton.querySelector("span")).toHaveClass(
      "hidden",
      "sm:inline",
    );

    fireEvent.click(screen.getByRole("button", { name: "common.moreActions" }));
    const compactMenu = await screen.findByTestId("web-provider-tools-menu");
    const compactActions = within(compactMenu);
    expect(
      compactActions.getByRole("button", { name: "skills.manage" }),
    ).toBeInTheDocument();
    expect(
      compactActions.getByRole("button", { name: "prompts.manage" }),
    ).toBeInTheDocument();
    expect(
      compactActions.getByRole("button", { name: "mcp.title" }),
    ).toBeInTheDocument();
  });

  it("hosts the Skills check-update action in the App toolbar", async () => {
    localStorage.setItem("cc-switch-last-view", "skills");
    const { default: App } = await import("@/App");
    renderApp(App);

    expect(
      await screen.findByTestId("unified-skills-panel"),
    ).toBeInTheDocument();
    const checkUpdatesButton = await screen.findByRole("button", {
      name: "skills.checkUpdates",
    });
    await waitFor(() => expect(checkUpdatesButton).toBeEnabled());

    fireEvent.click(checkUpdatesButton);
    expect(skillsPanelMocks.checkUpdates).toHaveBeenCalledTimes(1);
  });

  it("routes the Skills discover toolbar action through the panel guard", async () => {
    localStorage.setItem("cc-switch-last-view", "skills");
    const { default: App } = await import("@/App");
    renderApp(App);

    expect(
      await screen.findByTestId("unified-skills-panel"),
    ).toBeInTheDocument();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "skills.discover",
      }),
    );

    expect(skillsPanelMocks.openDiscovery).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("unified-skills-panel")).toBeInTheDocument();
  });
});
