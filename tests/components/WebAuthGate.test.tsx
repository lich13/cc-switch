import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import i18n from "i18next";
import { WebAuthGate } from "@/components/web/WebAuthGate";
import {
  getCsrfToken,
  isWebRuntime,
  webLogin,
  webMe,
  webPublicSettings,
} from "@/lib/runtime";

vi.mock("@/lib/runtime", () => ({
  getCsrfToken: vi.fn(),
  isWebRuntime: vi.fn(),
  webLogin: vi.fn(),
  webMe: vi.fn(),
  webPublicSettings: vi.fn(),
}));

describe("WebAuthGate", () => {
  beforeEach(() => {
    i18n.addResourceBundle(
      "zh",
      "translation",
      {
        webAuth: {
          subtitle: "登录云机管理面",
          username: "用户名",
          password: "密码",
          login: "登录",
          turnstileRequired: "请先完成 Turnstile 校验",
          turnstileUnavailable: "Turnstile 校验暂时不可用，请刷新后重试",
          turnstileLoadFailed: "加载 Turnstile 失败",
        },
      },
      true,
      true,
    );
    i18n.addResourceBundle(
      "en",
      "translation",
      {
        webAuth: {
          subtitle: "Sign in to manage this server",
          username: "Username",
          password: "Password",
          login: "Log in",
          turnstileRequired: "Complete Turnstile verification first",
          turnstileUnavailable:
            "Turnstile verification is unavailable. Refresh and try again.",
          turnstileLoadFailed: "Failed to load Turnstile",
        },
      },
      true,
      true,
    );
    void i18n.changeLanguage("zh");
    vi.mocked(isWebRuntime).mockReturnValue(true);
    vi.mocked(getCsrfToken).mockReturnValue(null);
    vi.mocked(webMe).mockRejectedValue(new Error("unauthorized"));
    vi.mocked(webLogin).mockResolvedValue({
      username: "admin",
      csrfToken: "csrf-token",
    });
    vi.mocked(webPublicSettings).mockResolvedValue({
      version: "3.16.2-20",
      production: true,
      appName: "CC Switch WebUI",
      turnstile_enabled: false,
      turnstile_required: false,
      turnstile_site_key: "site-key",
      turnstile_action: "login",
    });
  });

  it("csrfToken 缺失但 cookie 有效时通过 /me 恢复会话", async () => {
    vi.mocked(getCsrfToken).mockReturnValue(null);
    vi.mocked(webMe).mockResolvedValue({
      username: "admin",
      csrfToken: "restored-csrf-token",
    });

    render(
      <WebAuthGate>
        <div>app</div>
      </WebAuthGate>,
    );

    expect(await screen.findByText("app")).toBeInTheDocument();
    expect(webMe).toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "登录" })).toBeNull();
  });

  it("turnstile 关闭时保持普通网页登录", async () => {
    render(
      <WebAuthGate>
        <div>app</div>
      </WebAuthGate>,
    );

    fireEvent.change(await screen.findByLabelText("密码"), {
      target: { value: "secret" },
    });
    fireEvent.click(screen.getByRole("button", { name: "登录" }));

    await waitFor(() => {
      expect(webLogin).toHaveBeenCalledWith("admin", "secret", undefined);
    });
  });

  it("网页登录页跟随当前界面语言", async () => {
    await i18n.changeLanguage("en");

    render(
      <WebAuthGate>
        <div>app</div>
      </WebAuthGate>,
    );

    expect(
      await screen.findByText("Sign in to manage this server"),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Username")).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Log in" })).toBeInTheDocument();
    expect(screen.queryByText("登录云机管理面")).not.toBeInTheDocument();
  });

  it("turnstile 开启时使用 widget token 登录", async () => {
    vi.mocked(webPublicSettings).mockResolvedValue({
      version: "3.16.2-20",
      production: true,
      appName: "CC Switch WebUI",
      turnstile_enabled: true,
      turnstile_required: false,
      turnstile_site_key: "site-key",
      turnstile_action: "login",
    });

    render(
      <WebAuthGate>
        <div>app</div>
      </WebAuthGate>,
    );

    const widget = await screen.findByTestId("turnstile-widget");
    fireEvent(
      widget,
      new CustomEvent("cc-switch-turnstile-token", {
        detail: "turnstile-token",
      }),
    );

    fireEvent.change(screen.getByLabelText("密码"), {
      target: { value: "secret" },
    });
    fireEvent.click(screen.getByRole("button", { name: "登录" }));

    await waitFor(() => {
      expect(webLogin).toHaveBeenCalledWith(
        "admin",
        "secret",
        "turnstile-token",
      );
    });
  });
});
