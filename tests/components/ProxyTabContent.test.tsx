import { render, screen } from "@testing-library/react";
import "@testing-library/jest-dom";
import { describe, expect, it, vi } from "vitest";
import { ProxyTabContent } from "@/components/settings/ProxyTabContent";
import type { SettingsFormState } from "@/hooks/useSettings";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("framer-motion", () => ({
  motion: {
    div: ({ children, ...props }: any) => <div {...props}>{children}</div>,
  },
}));

vi.mock("@/hooks/useProxyStatus", () => ({
  useProxyStatus: () => ({
    isRunning: false,
    takeoverStatus: {},
    startProxyServer: vi.fn(),
    stopWithRestore: vi.fn(),
    isPending: false,
  }),
}));

vi.mock("@/components/ui/accordion", () => ({
  Accordion: ({ children }: any) => <div>{children}</div>,
  AccordionContent: ({ children }: any) => <div>{children}</div>,
  AccordionItem: ({ children }: any) => <section>{children}</section>,
  AccordionTrigger: ({ children }: any) => <div>{children}</div>,
}));

vi.mock("@/components/ui/tabs", () => ({
  Tabs: ({ children }: any) => <div>{children}</div>,
  TabsContent: ({ children }: any) => <div>{children}</div>,
  TabsList: ({ children }: any) => <div>{children}</div>,
  TabsTrigger: ({ children }: any) => <button type="button">{children}</button>,
}));

vi.mock("@/components/proxy", () => ({
  ProxyPanel: () => <div data-testid="proxy-panel" />,
}));

vi.mock("@/components/proxy/AutoFailoverConfigPanel", () => ({
  AutoFailoverConfigPanel: () => <div data-testid="auto-failover-panel" />,
}));

vi.mock("@/components/proxy/FailoverQueueManager", () => ({
  FailoverQueueManager: () => <div data-testid="failover-queue" />,
}));

vi.mock("@/components/settings/RectifierConfigPanel", () => ({
  RectifierConfigPanel: () => <div data-testid="rectifier-panel" />,
}));

vi.mock("@/components/settings/GlobalProxySettings", () => ({
  GlobalProxySettings: () => <div data-testid="global-proxy-settings" />,
}));

vi.mock("@/components/ConfirmDialog", () => ({
  ConfirmDialog: () => null,
}));

const makeSettings = (
  disableImageGeneration: SettingsFormState["disableImageGeneration"] = false,
): SettingsFormState =>
  ({
    showInTray: true,
    minimizeToTrayOnClose: true,
    language: "zh",
    disableImageGeneration,
  }) as SettingsFormState;

const renderProxyTab = (
  disableImageGeneration: SettingsFormState["disableImageGeneration"] = false,
) => {
  const onAutoSave = vi.fn().mockResolvedValue(undefined);
  render(
    <ProxyTabContent
      settings={makeSettings(disableImageGeneration)}
      onAutoSave={onAutoSave}
    />,
  );
  return { onAutoSave };
};

describe("ProxyTabContent", () => {
  it("does not render the legacy global chat image-generation toggle", () => {
    renderProxyTab();

    expect(screen.getByTestId("rectifier-panel")).toBeInTheDocument();
    expect(
      screen.queryByRole("switch", {
        name: "settings.disableImageGenerationInChat",
      }),
    ).not.toBeInTheDocument();
  });
});
