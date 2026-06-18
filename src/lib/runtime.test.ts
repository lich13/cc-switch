import { beforeEach, describe, expect, it, vi } from "vitest";
import { webFetch, webLogin } from "@/lib/runtime";

describe("webLogin", () => {
  beforeEach(() => {
    window.history.replaceState(null, "", "/cc-switch/");
    window.sessionStorage.clear();
  });

  it("将 turnstile token 发送给网页登录接口", async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ username: "admin", csrfToken: "csrf" }), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    );
    vi.stubGlobal("fetch", fetchMock);

    await webLogin("admin", "secret", "turnstile-token");

    expect(fetchMock).toHaveBeenCalledWith(
      "/cc-switch/api/auth/login",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          username: "admin",
          password: "secret",
          turnstile_token: "turnstile-token",
        }),
      }),
    );
  });
});

describe("webFetch", () => {
  beforeEach(() => {
    window.history.replaceState(null, "", "/cc-switch/");
    window.sessionStorage.clear();
  });

  it("为 Web 管理接口补齐部署前缀、same-origin 和 CSRF", async () => {
    window.sessionStorage.setItem("cc-switch-web-csrf", "csrf-token");
    const fetchMock = vi.fn().mockResolvedValue(
      new Response("{}", {
        status: 200,
      }),
    );
    vi.stubGlobal("fetch", fetchMock);

    await webFetch("/api/admin/providers/import", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: '{"providers":[]}',
    });

    expect(fetchMock).toHaveBeenCalledWith(
      "/cc-switch/api/admin/providers/import",
      expect.objectContaining({
        method: "POST",
        credentials: "same-origin",
        body: '{"providers":[]}',
      }),
    );
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = init.headers as Headers;
    expect(headers.get("content-type")).toBe("application/json");
    expect(headers.get("x-csrf-token")).toBe("csrf-token");
  });
});
