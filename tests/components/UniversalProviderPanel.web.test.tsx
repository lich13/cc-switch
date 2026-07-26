import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { http, HttpResponse } from "msw";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { UniversalProviderPanel } from "@/components/universal/UniversalProviderPanel";
import type { UniversalProvider } from "@/types";
import { server } from "../msw/server";
import {
  resetProviderState,
  setUniversalProviderForEdit,
  setUniversalProviders,
} from "../msw/state";

const toastErrorMock = vi.hoisted(() => vi.fn());

vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => toastErrorMock(...args),
    success: vi.fn(),
  },
}));

vi.mock("@/lib/runtime", async () => {
  const actual =
    await vi.importActual<typeof import("@/lib/runtime")>("@/lib/runtime");
  return { ...actual, isWebRuntime: () => true };
});

vi.mock("@/components/universal/UniversalProviderCard", () => ({
  UniversalProviderCard: ({ provider, onEdit }: any) => (
    <div>
      <output data-testid="universal-list-payload">
        {JSON.stringify(provider)}
      </output>
      <button onClick={() => onEdit(provider)}>edit-universal</button>
    </div>
  ),
}));

vi.mock("@/components/universal/UniversalProviderFormModal", () => ({
  UniversalProviderFormModal: ({
    isOpen,
    editingProvider,
    onClose,
    onExitComplete,
  }: any) => (
    <div data-testid="universal-form-shell" data-open={String(isOpen)}>
      <output data-testid="universal-edit-payload">
        {editingProvider ? JSON.stringify(editingProvider) : ""}
      </output>
      {isOpen ? <div data-testid="universal-edit-panel" /> : null}
      <button onClick={onClose}>close-universal-edit</button>
      <button onClick={() => onExitComplete?.()}>finish-universal-exit</button>
    </div>
  ),
}));

const maskedProvider: UniversalProvider = {
  id: "universal-1",
  name: "Universal One",
  providerType: "custom",
  apps: { claude: true, codex: true, gemini: true },
  baseUrl: "https://api.example.com",
  apiKey: "secret_configured",
  models: {},
};

const editDetail: UniversalProvider = {
  ...maskedProvider,
  apiKey: "sk-universal-web-detail",
};

describe("UniversalProviderPanel web edit boundary", () => {
  beforeEach(() => {
    resetProviderState();
    toastErrorMock.mockReset();
    window.localStorage.clear();
    setUniversalProviders({ [maskedProvider.id]: maskedProvider });
    setUniversalProviderForEdit(maskedProvider.id, editDetail);
  });

  it("keeps the list masked and clears on-demand detail after panel exit", async () => {
    render(<UniversalProviderPanel />);

    await waitFor(() => {
      expect(screen.getByTestId("universal-list-payload")).toHaveTextContent(
        "secret_configured",
      );
    });
    expect(screen.getByTestId("universal-list-payload")).not.toHaveTextContent(
      "sk-universal-web-detail",
    );

    fireEvent.click(screen.getByText("edit-universal"));

    await waitFor(() => {
      expect(screen.getByTestId("universal-edit-panel")).toBeInTheDocument();
      expect(screen.getByTestId("universal-edit-payload")).toHaveTextContent(
        "sk-universal-web-detail",
      );
    });
    expect(JSON.stringify(window.localStorage)).not.toContain(
      "sk-universal-web-detail",
    );

    fireEvent.click(screen.getByText("close-universal-edit"));
    expect(
      screen.queryByTestId("universal-edit-panel"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("universal-edit-payload")).toHaveTextContent(
      "sk-universal-web-detail",
    );

    fireEvent.click(screen.getByText("finish-universal-exit"));
    await waitFor(() => {
      expect(
        screen.getByTestId("universal-edit-payload"),
      ).toBeEmptyDOMElement();
    });
  });

  it("does not open the panel when the edit-detail request fails", async () => {
    server.use(
      http.post("http://tauri.local/get_universal_provider_for_edit", () =>
        HttpResponse.json({ error: "detail unavailable" }, { status: 500 }),
      ),
    );
    render(<UniversalProviderPanel />);

    await screen.findByText("edit-universal");
    fireEvent.click(screen.getByText("edit-universal"));

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalled());
    expect(
      screen.queryByTestId("universal-edit-panel"),
    ).not.toBeInTheDocument();
  });
});
