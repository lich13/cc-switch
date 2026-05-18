import type { HTMLAttributes, ReactNode } from "react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
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
  AccordionItem: ({ children }: { children: ReactNode }) => <div>{children}</div>,
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
  useProxyRoutingMode: (appType: string) => ({
    data: appType === "codex" ? "local_only" : "off",
  }),
}));

vi.mock("@/components/proxy", () => ({
  ProxyPanel: () => <div data-testid="proxy-panel" />,
}));

vi.mock("@/components/proxy/FailoverQueueManager", () => ({
  FailoverQueueManager: ({
    appType,
    disabled,
  }: {
    appType: string;
    disabled?: boolean;
  }) => (
    <div
      data-testid={`failover-queue-${appType}`}
      data-disabled={String(disabled)}
    />
  ),
}));

vi.mock("@/components/proxy/AutoFailoverConfigPanel", () => ({
  AutoFailoverConfigPanel: ({
    appType,
    disabled,
  }: {
    appType: string;
    disabled?: boolean;
  }) => (
    <div
      data-testid={`auto-failover-config-${appType}`}
      data-disabled={String(disabled)}
    />
  ),
}));

vi.mock("@/components/settings/RectifierConfigPanel", () => ({
  RectifierConfigPanel: () => <div />,
}));

vi.mock("@/components/settings/GlobalProxySettings", () => ({
  GlobalProxySettings: () => <div />,
}));

describe("ProxyTabContent failover routing availability", () => {
  it("keeps Codex failover controls enabled when pure local route is active", () => {
    const settings: SettingsFormState = {
      showInTray: true,
      minimizeToTrayOnClose: true,
      proxyConfirmed: true,
      failoverConfirmed: true,
      enableFailoverToggle: true,
      language: "zh",
    };

    render(
      <ProxyTabContent
        settings={settings}
        onAutoSave={vi.fn()}
      />,
    );

    expect(screen.getByTestId("failover-queue-codex")).toHaveAttribute(
      "data-disabled",
      "false",
    );
    expect(screen.getByTestId("auto-failover-config-codex")).toHaveAttribute(
      "data-disabled",
      "false",
    );
  });
});
