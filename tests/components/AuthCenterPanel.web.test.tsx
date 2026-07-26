import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AuthCenterPanel } from "@/components/settings/AuthCenterPanel";

const runtimeMocks = vi.hoisted(() => ({
  isWebRuntime: vi.fn(() => false),
}));

vi.mock("@/lib/runtime", () => ({
  isWebRuntime: () => runtimeMocks.isWebRuntime(),
}));

vi.mock("@/components/providers/forms/CopilotAuthSection", () => ({
  CopilotAuthSection: () => <div data-testid="copilot-auth" />,
}));
vi.mock("@/components/providers/forms/CodexOAuthSection", () => ({
  CodexOAuthSection: () => <div data-testid="codex-auth" />,
}));
vi.mock("@/components/providers/forms/XaiOAuthSection", () => ({
  XaiOAuthSection: () => <div data-testid="xai-auth" />,
}));
vi.mock("@/components/ProviderIcon", () => ({
  ProviderIcon: () => <span />,
}));

describe("AuthCenterPanel runtime boundary", () => {
  beforeEach(() => {
    runtimeMocks.isWebRuntime.mockReset();
  });

  it("hides xAI OAuth account management in WebUI", () => {
    runtimeMocks.isWebRuntime.mockReturnValue(true);
    render(<AuthCenterPanel />);

    expect(screen.getByTestId("copilot-auth")).toBeInTheDocument();
    expect(screen.getByTestId("codex-auth")).toBeInTheDocument();
    expect(screen.queryByTestId("xai-auth")).not.toBeInTheDocument();
    expect(screen.queryByText("xAI (Grok OAuth)")).not.toBeInTheDocument();
  });

  it("keeps xAI OAuth account management on desktop", () => {
    runtimeMocks.isWebRuntime.mockReturnValue(false);
    render(<AuthCenterPanel />);

    expect(screen.getByTestId("xai-auth")).toBeInTheDocument();
  });
});
