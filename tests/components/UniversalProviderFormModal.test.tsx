import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { UniversalProviderFormModal } from "@/components/universal/UniversalProviderFormModal";
import type { UniversalProvider } from "@/types";

vi.mock("@/components/common/FullScreenPanel", () => ({
  FullScreenPanel: ({ children, footer, onClose, onExitComplete }: any) => (
    <div>
      {children}
      {footer}
      <button onClick={onClose}>close-panel</button>
      <button onClick={() => onExitComplete?.()}>finish-panel-exit</button>
    </div>
  ),
}));

vi.mock("@/components/JsonEditor", () => ({
  default: () => <div data-testid="json-editor" />,
}));

vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => <span data-testid="provider-icon" />,
}));

const provider: UniversalProvider = {
  id: "universal-edit",
  name: "Universal Edit",
  providerType: "custom",
  apps: { claude: true, codex: true, gemini: true },
  baseUrl: "https://api.example.com",
  apiKey: "sk-visible-after-eye",
  models: {},
};

describe("UniversalProviderFormModal API key", () => {
  it("uses the shared masked input, reveals the real key, allows edits, and clears on exit", () => {
    const onSave = vi.fn();
    const onExitComplete = vi.fn();
    render(
      <UniversalProviderFormModal
        isOpen
        editingProvider={provider}
        onClose={vi.fn()}
        onExitComplete={onExitComplete}
        onSave={onSave}
      />,
    );

    const input = screen.getByLabelText("API Key") as HTMLInputElement;
    expect(input).toHaveAttribute("type", "password");
    expect(input).toHaveValue("sk-visible-after-eye");

    fireEvent.click(screen.getByRole("button", { name: "apiKeyInput.show" }));
    expect(input).toHaveAttribute("type", "text");

    fireEvent.change(input, { target: { value: "sk-updated-directly" } });
    expect(input).toHaveValue("sk-updated-directly");

    fireEvent.click(screen.getByText("finish-panel-exit"));
    expect(input).toHaveValue("");
    expect(onExitComplete).toHaveBeenCalledTimes(1);
  });
});
