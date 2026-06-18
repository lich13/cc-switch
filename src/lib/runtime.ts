export type UnlistenFn = () => void;

export interface RuntimeEvent<T> {
  payload: T;
}

type InvokeArgs = Record<string, unknown> | undefined;

const CSRF_STORAGE_KEY = "cc-switch-web-csrf";
const TEST_HOME_DIR = "/home/mock";

type TestEventHandler = (event: RuntimeEvent<unknown>) => void;
type TestRuntimeGlobal = typeof globalThis & {
  __CC_SWITCH_TEST_EVENT_LISTENERS__?: Map<string, Set<TestEventHandler>>;
  __CC_SWITCH_TEST_INVOKE__?: (
    command: string,
    args?: InvokeArgs,
  ) => Promise<unknown> | unknown;
};

function isForcedWebRuntimeForTests(): boolean {
  if (import.meta.env.MODE !== "test" || typeof window === "undefined") {
    return false;
  }
  return Boolean(
    (window as Window & { __CC_SWITCH_WEB_RUNTIME__?: boolean })
      .__CC_SWITCH_WEB_RUNTIME__,
  );
}

export function isDesktopRuntime(): boolean {
  if (typeof window === "undefined") {
    return false;
  }
  if (isForcedWebRuntimeForTests()) {
    return false;
  }
  return "__TAURI_INTERNALS__" in window || import.meta.env.MODE === "test";
}

export function isWebRuntime(): boolean {
  return !isDesktopRuntime();
}

function isTauriAvailable(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function isTestRuntimeWithoutTauri(): boolean {
  return (
    import.meta.env.MODE === "test" &&
    typeof window !== "undefined" &&
    !isForcedWebRuntimeForTests() &&
    !isTauriAvailable()
  );
}

function getTestEventListeners(): Map<string, Set<TestEventHandler>> {
  const target = globalThis as TestRuntimeGlobal;
  if (!target.__CC_SWITCH_TEST_EVENT_LISTENERS__) {
    target.__CC_SWITCH_TEST_EVENT_LISTENERS__ = new Map();
  }
  return target.__CC_SWITCH_TEST_EVENT_LISTENERS__;
}

function listenInTestRuntime<T>(
  eventName: string,
  handler: (event: RuntimeEvent<T>) => void,
): UnlistenFn {
  const listeners = getTestEventListeners();
  if (!listeners.has(eventName)) {
    listeners.set(eventName, new Set());
  }
  const wrapped: TestEventHandler = (event) =>
    handler(event as RuntimeEvent<T>);
  listeners.get(eventName)!.add(wrapped);
  return () => {
    listeners.get(eventName)?.delete(wrapped);
  };
}

export function getCsrfToken(): string | null {
  if (typeof window === "undefined") return null;
  return window.sessionStorage.getItem(CSRF_STORAGE_KEY);
}

function setCsrfToken(token: string | null): void {
  if (typeof window === "undefined") return;
  if (token) {
    window.sessionStorage.setItem(CSRF_STORAGE_KEY, token);
  } else {
    window.sessionStorage.removeItem(CSRF_STORAGE_KEY);
  }
}

function webPath(path: string): string {
  if (typeof window === "undefined") return path;
  const pathname = window.location.pathname;
  const marker = pathname.indexOf("/cc-switch/");
  if (marker < 0) return path;
  const base = pathname.slice(0, marker + "/cc-switch".length);
  if (path === base || path.startsWith(`${base}/`)) {
    return path;
  }
  return `${base}${path.startsWith("/") ? path : `/${path}`}`;
}

async function webRpc<T>(command: string, args?: InvokeArgs): Promise<T> {
  const headers: Record<string, string> = {
    "content-type": "application/json",
  };
  const csrf = getCsrfToken();
  if (csrf) {
    headers["x-csrf-token"] = csrf;
  }

  const response = await fetch(
    webPath(`/api/admin/rpc/${encodeURIComponent(command)}`),
    {
      method: "POST",
      credentials: "same-origin",
      headers,
      body: JSON.stringify(args ?? {}),
    },
  );

  if (response.status === 401) {
    setCsrfToken(null);
    window.dispatchEvent(new CustomEvent("cc-switch-web-auth-required"));
  }

  const contentType = response.headers.get("content-type") ?? "";
  const body = contentType.includes("application/json")
    ? await response.json().catch(() => null)
    : await response.text().catch(() => "");

  if (!response.ok) {
    const message =
      body && typeof body === "object" && "error" in body
        ? String((body as { error?: unknown }).error)
        : typeof body === "string" && body
          ? body
          : `HTTP ${response.status}`;
    throw new Error(message);
  }

  return (
    body && typeof body === "object" && "data" in body
      ? (body as { data: T }).data
      : body
  ) as T;
}

function normalizeWebFetchPath(input: string | URL): string {
  if (typeof window === "undefined") {
    return String(input);
  }

  const url = new URL(String(input), window.location.origin);
  if (url.origin !== window.location.origin) {
    throw new Error("Cross-origin web requests are not allowed");
  }
  return `${url.pathname}${url.search}${url.hash}`;
}

export async function webFetch(
  input: string | URL,
  init: RequestInit = {},
): Promise<Response> {
  const headers = new Headers(init.headers);
  const csrf = getCsrfToken();
  if (csrf && !headers.has("x-csrf-token")) {
    headers.set("x-csrf-token", csrf);
  }

  const response = await fetch(webPath(normalizeWebFetchPath(input)), {
    ...init,
    credentials: "same-origin",
    headers,
  });

  if (response.status === 401) {
    setCsrfToken(null);
    window.dispatchEvent(new CustomEvent("cc-switch-web-auth-required"));
  }

  return response;
}

async function testRpc<T>(command: string, args?: InvokeArgs): Promise<T> {
  const response = await fetch(`http://tauri.local/${command}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(args ?? {}),
  });

  if (!response.ok) {
    const text = await response.text().catch(() => "");
    throw new Error(text || `Invoke failed for ${command}`);
  }

  const text = await response.text().catch(() => "");
  if (!text) return undefined as T;
  try {
    return JSON.parse(text) as T;
  } catch {
    return text as T;
  }
}

export async function invoke<T = unknown>(
  command: string,
  args?: InvokeArgs,
): Promise<T> {
  if (isDesktopRuntime()) {
    try {
      const tauri = await import("@tauri-apps/api/core");
      return await tauri.invoke<T>(command, args);
    } catch (error) {
      if (isTestRuntimeWithoutTauri()) {
        const testInvoke = (globalThis as TestRuntimeGlobal)
          .__CC_SWITCH_TEST_INVOKE__;
        if (testInvoke) {
          return (await testInvoke(command, args)) as T;
        }
        return testRpc<T>(command, args);
      }
      throw error;
    }
  }
  return webRpc<T>(command, args);
}

export async function listen<T>(
  eventName: string,
  handler: (event: RuntimeEvent<T>) => void,
): Promise<UnlistenFn> {
  if (isTestRuntimeWithoutTauri()) {
    return listenInTestRuntime(eventName, handler);
  }
  if (isDesktopRuntime()) {
    const tauri = await import("@tauri-apps/api/event");
    return tauri.listen<T>(eventName, (event) =>
      handler({ payload: event.payload }),
    );
  }
  return () => undefined;
}

export async function getVersion(): Promise<string> {
  if (!isDesktopRuntime()) {
    const publicSettings = await webPublicSettings().catch(() => null);
    return publicSettings?.version ?? "";
  }
  const tauri = await import("@tauri-apps/api/app");
  return tauri.getVersion();
}

export async function showMessage(
  text: string,
  options?: { title?: string; kind?: "info" | "warning" | "error" },
): Promise<void> {
  if (isDesktopRuntime()) {
    const dialog = await import("@tauri-apps/plugin-dialog");
    await dialog.message(text, options);
    return;
  }
  window.alert(options?.title ? `${options.title}\n\n${text}` : text);
}

export async function exitApp(code = 0): Promise<void> {
  if (isDesktopRuntime()) {
    const process = await import("@tauri-apps/plugin-process");
    await process.exit(code);
    return;
  }
  console.warn(`exitApp ignored in web runtime: ${code}`);
}

export function getCurrentWindow() {
  const call = async <T>(
    method: (
      windowApi: ReturnType<
        typeof import("@tauri-apps/api/window").getCurrentWindow
      >,
    ) => Promise<T>,
    fallback: T,
  ): Promise<T> => {
    if (!isDesktopRuntime() || isTestRuntimeWithoutTauri()) return fallback;
    const windowApi = await import("@tauri-apps/api/window");
    return method(windowApi.getCurrentWindow());
  };

  return {
    isMaximized: () => call((w) => w.isMaximized(), false),
    onResized: (
      handler: Parameters<
        ReturnType<
          typeof import("@tauri-apps/api/window").getCurrentWindow
        >["onResized"]
      >[0],
    ) =>
      call(
        (w) => w.onResized(handler),
        () => undefined,
      ),
    setDecorations: (enabled: boolean) =>
      call(async (w) => {
        await w.setDecorations(enabled);
      }, undefined),
    minimize: () =>
      call(async (w) => {
        await w.minimize();
      }, undefined),
    maximize: () =>
      call(async (w) => {
        await w.maximize();
      }, undefined),
    unmaximize: () =>
      call(async (w) => {
        await w.unmaximize();
      }, undefined),
    toggleMaximize: () =>
      call(async (w) => {
        await w.toggleMaximize();
      }, undefined),
    close: () =>
      call(async (w) => {
        await w.close();
      }, undefined),
  };
}

export async function homeDir(): Promise<string> {
  if (isTestRuntimeWithoutTauri()) {
    return TEST_HOME_DIR;
  }
  if (isDesktopRuntime()) {
    const path = await import("@tauri-apps/api/path");
    return path.homeDir();
  }
  return "/";
}

export async function join(...paths: string[]): Promise<string> {
  if (isTestRuntimeWithoutTauri()) {
    return paths.filter(Boolean).join("/").replace(/\/+/g, "/");
  }
  if (isDesktopRuntime()) {
    const path = await import("@tauri-apps/api/path");
    return path.join(...paths);
  }
  return paths.filter(Boolean).join("/").replace(/\/+/g, "/");
}

export interface WebPublicSettings {
  version: string;
  production: boolean;
  appName: string;
  adminInitialized?: boolean;
  admin_configured?: boolean;
  turnstile_enabled?: boolean;
  turnstile_required?: boolean;
  turnstile_site_key?: string;
  turnstile_action?: string;
}

export interface WebMe {
  username: string;
  csrfToken: string;
}

export async function webPublicSettings(): Promise<WebPublicSettings> {
  const response = await fetch(webPath("/api/public/settings"), {
    credentials: "same-origin",
  });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
}

export async function webMe(): Promise<WebMe> {
  const response = await fetch(webPath("/api/admin/me"), {
    credentials: "same-origin",
  });
  if (!response.ok) {
    setCsrfToken(null);
    throw new Error(`HTTP ${response.status}`);
  }
  const me = (await response.json()) as WebMe;
  setCsrfToken(me.csrfToken);
  return me;
}

export async function webLogin(
  username: string,
  password: string,
  turnstileToken?: string,
): Promise<WebMe> {
  const requestBody: {
    username: string;
    password: string;
    turnstile_token?: string;
  } = { username, password };
  if (turnstileToken) {
    requestBody.turnstile_token = turnstileToken;
  }

  const response = await fetch(webPath("/api/auth/login"), {
    method: "POST",
    credentials: "same-origin",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(requestBody),
  });
  const body = await response.json().catch(() => null);
  if (!response.ok) {
    throw new Error(
      body && typeof body === "object" && "error" in body
        ? String((body as { error?: unknown }).error)
        : `HTTP ${response.status}`,
    );
  }
  const me = body as WebMe;
  setCsrfToken(me.csrfToken);
  return me;
}

export async function webLogout(): Promise<void> {
  const headers: Record<string, string> = {};
  const csrf = getCsrfToken();
  if (csrf) {
    headers["x-csrf-token"] = csrf;
  }
  await fetch(webPath("/api/auth/logout"), {
    method: "POST",
    credentials: "same-origin",
    headers,
  }).catch(() => undefined);
  setCsrfToken(null);
  window.dispatchEvent(new CustomEvent("cc-switch-web-auth-required"));
}
