import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { streamCheckProvider } from "@/lib/api/connectivity-check";

describe("connectivity check web runtime", () => {
  beforeEach(() => {
    window.history.replaceState(null, "", "/cc-switch/");
    (
      window as Window & { __CC_SWITCH_WEB_RUNTIME__?: boolean }
    ).__CC_SWITCH_WEB_RUNTIME__ = true;
    window.sessionStorage.clear();
  });

  afterEach(() => {
    delete (window as Window & { __CC_SWITCH_WEB_RUNTIME__?: boolean })
      .__CC_SWITCH_WEB_RUNTIME__;
    vi.unstubAllGlobals();
  });

  it("routes provider checks through the prefixed web RPC endpoint", async () => {
    const result = {
      status: "operational" as const,
      success: true,
      message: "reachable",
      responseTimeMs: 42,
      testedAt: 123,
      retryCount: 0,
    };
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ data: result }), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    );
    vi.stubGlobal("fetch", fetchMock);

    await expect(streamCheckProvider("codex", "provider-1")).resolves.toEqual(
      result,
    );

    expect(fetchMock).toHaveBeenCalledWith(
      "/cc-switch/api/admin/rpc/stream_check_provider",
      expect.objectContaining({
        method: "POST",
        credentials: "same-origin",
        body: JSON.stringify({
          appType: "codex",
          providerId: "provider-1",
        }),
      }),
    );
  });
});
