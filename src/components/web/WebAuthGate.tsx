import { useEffect, useRef, useState } from "react";
import { Loader2, LockKeyhole } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  isWebRuntime,
  webLogin,
  webMe,
  webPublicSettings,
  type WebPublicSettings,
} from "@/lib/runtime";

interface WebAuthGateProps {
  children: React.ReactNode;
}

type TurnstileApi = {
  render: (
    container: HTMLElement,
    options: {
      sitekey: string;
      action?: string;
      callback: (token: string) => void;
      "expired-callback": () => void;
      "error-callback": () => void;
    },
  ) => string;
  reset: (widgetId?: string) => void;
  remove?: (widgetId: string) => void;
};

declare global {
  interface Window {
    turnstile?: TurnstileApi;
  }
}

const TURNSTILE_SCRIPT_ID = "cc-switch-turnstile-script";
const TURNSTILE_SCRIPT_SRC =
  "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";

let turnstileScriptPromise: Promise<void> | null = null;

function loadTurnstileScript(): Promise<void> {
  if (typeof window === "undefined") {
    return Promise.reject(new Error("window is unavailable"));
  }
  if (window.turnstile) {
    return Promise.resolve();
  }
  if (turnstileScriptPromise) {
    return turnstileScriptPromise;
  }

  turnstileScriptPromise = new Promise((resolve, reject) => {
    const existing = document.getElementById(
      TURNSTILE_SCRIPT_ID,
    ) as HTMLScriptElement | null;
    if (existing) {
      existing.addEventListener("load", () => resolve(), { once: true });
      existing.addEventListener(
        "error",
        () => reject(new Error("加载 Turnstile 失败")),
        { once: true },
      );
      return;
    }

    const script = document.createElement("script");
    script.id = TURNSTILE_SCRIPT_ID;
    script.src = TURNSTILE_SCRIPT_SRC;
    script.async = true;
    script.defer = true;
    script.addEventListener("load", () => resolve(), { once: true });
    script.addEventListener(
      "error",
      () => reject(new Error("加载 Turnstile 失败")),
      { once: true },
    );
    document.head.appendChild(script);
  });

  return turnstileScriptPromise;
}

function TurnstileWidget({
  siteKey,
  action,
  resetKey,
  onToken,
  onError,
}: {
  siteKey: string;
  action?: string;
  resetKey: number;
  onToken: (token: string | null) => void;
  onError: (message: string | null) => void;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!siteKey || !containerRef.current) {
      return;
    }

    let cancelled = false;
    let widgetId: string | null = null;
    const container = containerRef.current;

    const handleTestToken = (event: Event) => {
      const token = (event as CustomEvent<string>).detail;
      onError(null);
      onToken(token || null);
    };
    container.addEventListener("cc-switch-turnstile-token", handleTestToken);

    void loadTurnstileScript()
      .then(() => {
        if (cancelled || !window.turnstile || !containerRef.current) {
          return;
        }
        containerRef.current.innerHTML = "";
        widgetId = window.turnstile.render(containerRef.current, {
          sitekey: siteKey,
          action,
          callback: (token) => {
            onError(null);
            onToken(token);
          },
          "expired-callback": () => {
            onToken(null);
          },
          "error-callback": () => {
            onToken(null);
            onError("Turnstile 校验暂时不可用，请刷新后重试");
          },
        });
      })
      .catch((err) => {
        if (!cancelled) {
          onError(err instanceof Error ? err.message : String(err));
        }
      });

    return () => {
      cancelled = true;
      container.removeEventListener(
        "cc-switch-turnstile-token",
        handleTestToken,
      );
      if (widgetId && window.turnstile?.remove) {
        window.turnstile.remove(widgetId);
      }
    };
  }, [action, onError, onToken, resetKey, siteKey]);

  return (
    <div
      ref={containerRef}
      data-testid="turnstile-widget"
      className="min-h-16"
    />
  );
}

export function WebAuthGate({ children }: WebAuthGateProps) {
  const [checking, setChecking] = useState(isWebRuntime());
  const [authenticated, setAuthenticated] = useState(!isWebRuntime());
  const [publicSettings, setPublicSettings] =
    useState<WebPublicSettings | null>(null);
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [turnstileToken, setTurnstileToken] = useState<string | null>(null);
  const [turnstileResetKey, setTurnstileResetKey] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  const turnstileEnabled = Boolean(
    publicSettings?.turnstile_enabled && publicSettings.turnstile_site_key,
  );

  const refreshSession = async () => {
    if (!isWebRuntime()) {
      setAuthenticated(true);
      setChecking(false);
      return;
    }
    try {
      setPublicSettings(await webPublicSettings());
    } catch {
      setPublicSettings(null);
    }
    setChecking(true);
    try {
      await webMe();
      setAuthenticated(true);
    } catch {
      setAuthenticated(false);
    } finally {
      setChecking(false);
    }
  };

  useEffect(() => {
    void refreshSession();
    const handler = () => {
      setAuthenticated(false);
    };
    window.addEventListener("cc-switch-web-auth-required", handler);
    return () => {
      window.removeEventListener("cc-switch-web-auth-required", handler);
    };
  }, []);

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setError(null);
    if (turnstileEnabled && !turnstileToken) {
      setError("请先完成 Turnstile 校验");
      return;
    }
    setSubmitting(true);
    try {
      await webLogin(username.trim(), password, turnstileToken ?? undefined);
      setPassword("");
      setTurnstileToken(null);
      setAuthenticated(true);
    } catch (err) {
      if (turnstileEnabled) {
        setTurnstileToken(null);
        setTurnstileResetKey((value) => value + 1);
      }
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  if (!isWebRuntime() || authenticated) {
    return <>{children}</>;
  }

  if (checking) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-background text-foreground">
        <Loader2 className="h-6 w-6 animate-spin text-orange-500" />
      </div>
    );
  }

  return (
    <main className="flex min-h-screen items-center justify-center bg-background px-4 text-foreground">
      <form
        onSubmit={handleSubmit}
        className="w-full max-w-sm rounded-lg border bg-card p-6 shadow-sm"
      >
        <div className="mb-6 flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-md bg-orange-500/10 text-orange-600 dark:text-orange-400">
            <LockKeyhole className="h-5 w-5" />
          </div>
          <div>
            <h1 className="text-lg font-semibold">CC Switch WebUI</h1>
            <p className="text-sm text-muted-foreground">登录云机管理面</p>
          </div>
        </div>

        <div className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="webui-username">用户名</Label>
            <Input
              id="webui-username"
              autoComplete="username"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="webui-password">密码</Label>
            <Input
              id="webui-password"
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
            />
          </div>
          {turnstileEnabled && (
            <TurnstileWidget
              siteKey={publicSettings!.turnstile_site_key!}
              action={publicSettings?.turnstile_action}
              resetKey={turnstileResetKey}
              onToken={setTurnstileToken}
              onError={setError}
            />
          )}
          {error && (
            <p className="rounded-md border border-red-500/20 bg-red-500/10 px-3 py-2 text-sm text-red-600 dark:text-red-400">
              {error}
            </p>
          )}
          <Button
            type="submit"
            className="w-full bg-orange-500 text-white hover:bg-orange-600"
            disabled={
              submitting ||
              !username.trim() ||
              !password ||
              (turnstileEnabled && !turnstileToken)
            }
          >
            {submitting && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            登录
          </Button>
        </div>
      </form>
    </main>
  );
}
