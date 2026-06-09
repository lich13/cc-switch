import type { HTMLAttributes, ReactNode } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom";
import { ProxyTabContent } from "@/components/settings/ProxyTabContent";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string } | string) => {
      if (typeof options === "object" && options.defaultValue) {
        return options.defaultValue;
      }
      if (typeof options === "string") {
        return options;
      }
      return key;
    },
  }),
}));

vi.mock("framer-motion", () => ({
  motion: {
    div: ({ children, ...props }: HTMLAttributes<HTMLDivElement>) => (
      <div {...props}>{children}</div>
    ),
  },
}));

vi.mock("@/components/ui/accordion", () => ({
  Accordion: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  AccordionItem: ({
    children,
    value,
  }: {
    children: ReactNode;
    value: string;
  }) => <section data-testid={`accordion-${value}`}>{children}</section>,
  AccordionTrigger: ({ children }: { children: ReactNode }) => (
    <button type="button">{children}</button>
  ),
  AccordionContent: ({ children }: { children: ReactNode }) => (
    <div>{children}</div>
  ),
}));

vi.mock("@/components/ui/tabs", () => ({
  Tabs: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  TabsList: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  TabsTrigger: ({ children }: { children: ReactNode }) => (
    <button type="button">{children}</button>
  ),
  TabsContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}));

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    isRunning: true,
    takeoverStatus: {
      claude: false,
      codex: false,
      gemini: false,
    },
    startProxyServer: vi.fn(),
    stopWithRestore: vi.fn(),
    isPending: false,
  }),
}));

vi.mock("@/lib/query/proxy", () => ({
  useProxyRoutingMode: () => ({ data: "off" }),
}));

vi.mock("@/components/proxy", () => ({
  ProxyPanel: () => <div />,
}));

vi.mock("@/components/proxy/FailoverQueueManager", () => ({
  FailoverQueueManager: () => <div />,
}));

vi.mock("@/components/proxy/AutoFailoverConfigPanel", () => ({
  AutoFailoverConfigPanel: () => <div />,
}));

vi.mock("@/components/settings/RectifierConfigPanel", () => ({
  RectifierConfigPanel: () => <div data-testid="rectifier-config-panel" />,
}));

vi.mock("@/components/settings/GlobalProxySettings", () => ({
  GlobalProxySettings: () => <div />,
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: () => null,
}));

const createSettings = (
  overrides: Partial<SettingsFormState> = {},
): SettingsFormState => ({
  showInTray: true,
  minimizeToTrayOnClose: true,
  proxyConfirmed: true,
  failoverConfirmed: true,
  enableFailoverToggle: true,
  disableImageGeneration: false,
  language: "zh",
  ...overrides,
});

describe("ProxyTabContent chat image generation controls", () => {
  it("renders the toggle disabled by default in the rectifier section before the rectifier panel", () => {
    render(
      <ProxyTabContent
        settings={createSettings()}
        onAutoSave={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    const rectifierSection = screen.getByTestId("accordion-rectifier");
    const toggle = within(rectifierSection).getByRole("switch", {
      name: "settings.disableImageGenerationInChat",
    });

    expect(toggle).not.toBeChecked();
    expect(
      toggle.compareDocumentPosition(
        within(rectifierSection).getByTestId("rectifier-config-panel"),
      ) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("treats chat and legacy true values as enabled", () => {
    const { rerender } = render(
      <ProxyTabContent
        settings={createSettings({ disableImageGeneration: "chat" })}
        onAutoSave={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    expect(
      screen.getByRole("switch", {
        name: "settings.disableImageGenerationInChat",
      }),
    ).toBeChecked();

    rerender(
      <ProxyTabContent
        settings={createSettings({ disableImageGeneration: true })}
        onAutoSave={vi.fn().mockResolvedValue(undefined)}
      />,
    );

    expect(
      screen.getByRole("switch", {
        name: "settings.disableImageGenerationInChat",
      }),
    ).toBeChecked();
  });

  it("writes false when disabled and chat when re-enabled from a legacy true load", () => {
    const onAutoSave = vi.fn().mockResolvedValue(undefined);
    const { rerender } = render(
      <ProxyTabContent
        settings={createSettings({ disableImageGeneration: true })}
        onAutoSave={onAutoSave}
      />,
    );

    fireEvent.click(
      screen.getByRole("switch", {
        name: "settings.disableImageGenerationInChat",
      }),
    );

    expect(onAutoSave).toHaveBeenCalledWith({
      disableImageGeneration: false,
    });

    rerender(
      <ProxyTabContent
        settings={createSettings({ disableImageGeneration: false })}
        onAutoSave={onAutoSave}
      />,
    );

    fireEvent.click(
      screen.getByRole("switch", {
        name: "settings.disableImageGenerationInChat",
      }),
    );

    expect(onAutoSave).toHaveBeenLastCalledWith({
      disableImageGeneration: "chat",
    });
  });
});
