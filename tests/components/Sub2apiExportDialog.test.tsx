import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Sub2apiExportDialog } from "@/components/settings/Sub2apiExportDialog";
import type { Sub2apiExportCandidate } from "@/lib/api/settings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/components/ui/dialog", () => ({
  Dialog: ({ open, children }: any) =>
    open ? <div data-testid="sub2api-dialog">{children}</div> : null,
  DialogContent: ({ children, className }: any) => (
    <div data-testid="sub2api-dialog-content" className={className}>
      {children}
    </div>
  ),
  DialogHeader: ({ children }: any) => <div>{children}</div>,
  DialogFooter: ({ children }: any) => <div>{children}</div>,
  DialogTitle: ({ children }: any) => <h2>{children}</h2>,
  DialogDescription: ({ children }: any) => <p>{children}</p>,
}));

vi.mock("@/components/ui/checkbox", () => ({
  Checkbox: ({ checked, onCheckedChange, "aria-label": ariaLabel }: any) => (
    <input
      type="checkbox"
      aria-label={ariaLabel}
      checked={Boolean(checked)}
      onChange={(event) => onCheckedChange?.(event.currentTarget.checked)}
    />
  ),
}));

vi.mock("@/components/ui/scroll-area", () => ({
  ScrollArea: ({ children, className }: any) => (
    <div data-testid="sub2api-scroll-area" className={className}>
      {children}
    </div>
  ),
}));

const candidates: Sub2apiExportCandidate[] = [
  {
    appType: "claude",
    providerId: "anthropic",
    name: "Anthropic",
    baseUrl: "https://anthropic.example",
  },
  {
    appType: "codex",
    providerId: "openrouter",
    name: "OpenRouter",
    baseUrl: "https://openrouter.example",
  },
];

const renderDialog = (
  overrides: Partial<React.ComponentProps<typeof Sub2apiExportDialog>> = {},
) => {
  const props = {
    open: true,
    candidates,
    selectedProviders: [],
    isExporting: false,
    onOpenChange: vi.fn(),
    onToggleProvider: vi.fn(),
    onSelectAll: vi.fn(),
    onClearSelection: vi.fn(),
    onConfirm: vi.fn(),
    ...overrides,
  };

  render(<Sub2apiExportDialog {...props} />);
  return props;
};

describe("Sub2apiExportDialog", () => {
  it("defaults every candidate checkbox to unchecked and disables export", () => {
    renderDialog();

    expect(screen.getByLabelText("Anthropic")).not.toBeChecked();
    expect(screen.getByLabelText("OpenRouter")).not.toBeChecked();
    expect(
      screen.getByRole("button", {
        name: "settings.sub2apiExportDialog.export",
      }),
    ).toBeDisabled();
  });

  it("renders only the candidates provided by the backend", () => {
    renderDialog({
      candidates: [candidates[0]],
    });

    expect(screen.getByText("Anthropic")).toBeInTheDocument();
    expect(screen.queryByText("OpenRouter")).not.toBeInTheDocument();
  });

  it("select all and clear buttons call their handlers", () => {
    const props = renderDialog();

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.sub2apiExportDialog.selectAll",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.sub2apiExportDialog.clear",
      }),
    );

    expect(props.onSelectAll).toHaveBeenCalledTimes(1);
    expect(props.onClearSelection).toHaveBeenCalledTimes(1);
  });

  it("checks selected accounts and clears them through clear handler", () => {
    const props = renderDialog({
      selectedProviders: [{ appType: "claude", providerId: "anthropic" }],
    });

    expect(screen.getByLabelText("Anthropic")).toBeChecked();
    expect(screen.getByLabelText("OpenRouter")).not.toBeChecked();

    fireEvent.click(screen.getByLabelText("Anthropic"));

    expect(props.onToggleProvider).toHaveBeenCalledWith(
      { appType: "claude", providerId: "anthropic" },
      false,
    );
  });

  it("exports selected accounts and keeps export disabled when selection is empty", () => {
    const props = renderDialog({
      selectedProviders: [{ appType: "codex", providerId: "openrouter" }],
    });

    const footer = screen.getByRole("button", {
      name: "settings.sub2apiExportDialog.export",
    }).parentElement!;

    expect(
      within(footer).getByRole("button", {
        name: "settings.sub2apiExportDialog.export",
      }),
    ).not.toBeDisabled();

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.sub2apiExportDialog.export",
      }),
    );

    expect(props.onConfirm).toHaveBeenCalledTimes(1);
  });

  it("keeps long candidate lists inside a scroll area without pushing out the footer", () => {
    const longCandidates: Sub2apiExportCandidate[] = Array.from(
      { length: 20 },
      (_, index) => ({
        appType: index % 2 === 0 ? "claude" : "codex",
        providerId: `provider-${index + 1}`,
        name: `Provider ${index + 1}`,
        baseUrl: `https://provider-${index + 1}.example`,
      }),
    );

    renderDialog({ candidates: longCandidates });

    expect(screen.getByText("Provider 1")).toBeInTheDocument();
    expect(screen.getByText("Provider 20")).toBeInTheDocument();

    const scrollArea = screen.getByTestId("sub2api-scroll-area");
    expect(screen.getByTestId("sub2api-dialog-content")).toHaveClass(
      "max-h-[90vh]",
      "min-h-0",
      "overflow-hidden",
    );
    expect(scrollArea.parentElement).toHaveClass(
      "flex-1",
      "min-h-0",
      "overflow-hidden",
    );
    expect(scrollArea.className).toContain("h-[min(45dvh,28rem)]");
    expect(scrollArea.className).toContain("min-h-0");
    expect(scrollArea.className).not.toContain("flex-1");

    const exportButton = screen.getByRole("button", {
      name: "settings.sub2apiExportDialog.export",
    });
    expect(exportButton).toBeDisabled();
    expect(scrollArea).not.toContainElement(exportButton);
  });
});
