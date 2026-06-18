import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ProviderImportExportSection } from "@/components/settings/ProviderImportExportSection";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

const baseProps = {
  status: "idle" as const,
  errorMessage: null,
  isImporting: false,
  isExporting: false,
  onImport: vi.fn(),
  onExport: vi.fn(),
  onExportSub2api: vi.fn(),
};

describe("ProviderImportExportSection", () => {
  it("renders sub2api export action and calls callback", () => {
    const onExportSub2api = vi.fn();

    render(
      <ProviderImportExportSection
        {...baseProps}
        onExportSub2api={onExportSub2api}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.exportProvidersSub2api",
      }),
    );

    expect(onExportSub2api).toHaveBeenCalledTimes(1);
  });

  it("disables sub2api export while another provider transfer is running", () => {
    render(<ProviderImportExportSection {...baseProps} isExporting />);

    expect(
      screen.getByRole("button", {
        name: "settings.exportingProviders",
      }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", {
        name: "settings.exportProvidersSub2api",
      }),
    ).toBeDisabled();
  });
});
