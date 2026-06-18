import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useProviderImportExport } from "@/hooks/useProviderImportExport";

const toastSuccessMock = vi.fn();
const toastErrorMock = vi.fn();

vi.mock("sonner", () => ({
  toast: {
    success: (...args: unknown[]) => toastSuccessMock(...args),
    error: (...args: unknown[]) => toastErrorMock(...args),
  },
}));

const isWebRuntimeMock = vi.fn();

vi.mock("@/lib/runtime", () => ({
  isWebRuntime: () => isWebRuntimeMock(),
}));

const openFileDialogMock = vi.fn();
const saveFileDialogMock = vi.fn();
const openProvidersFileDialogMock = vi.fn();
const saveProvidersFileDialogMock = vi.fn();
const importProvidersFromFileMock = vi.fn();
const exportProvidersToFileMock = vi.fn();
const exportProvidersSub2apiToFileMock = vi.fn();
const importProvidersFromContentMock = vi.fn();
const downloadProvidersExportMock = vi.fn();
const downloadProvidersSub2apiExportMock = vi.fn();
const getProvidersSub2apiExportCandidatesMock = vi.fn();

const sub2apiCandidates = [
  {
    appType: "claude",
    providerId: "anthropic",
    name: "Anthropic",
    baseUrl: "https://anthropic.example",
  },
  {
    appType: "codex",
    providerId: "openrouter",
    name: "OpenRouter",
    baseUrl: "https://openrouter.example",
  },
];

vi.mock("@/lib/api", () => ({
  settingsApi: {
    openFileDialog: (...args: unknown[]) => openFileDialogMock(...args),
    saveFileDialog: (...args: unknown[]) => saveFileDialogMock(...args),
    openProvidersFileDialog: (...args: unknown[]) =>
      openProvidersFileDialogMock(...args),
    saveProvidersFileDialog: (...args: unknown[]) =>
      saveProvidersFileDialogMock(...args),
    importProvidersFromFile: (...args: unknown[]) =>
      importProvidersFromFileMock(...args),
    exportProvidersToFile: (...args: unknown[]) =>
      exportProvidersToFileMock(...args),
    exportProvidersSub2apiToFile: (...args: unknown[]) =>
      exportProvidersSub2apiToFileMock(...args),
    importProvidersFromContent: (...args: unknown[]) =>
      importProvidersFromContentMock(...args),
    downloadProvidersExport: (...args: unknown[]) =>
      downloadProvidersExportMock(...args),
    downloadProvidersSub2apiExport: (...args: unknown[]) =>
      downloadProvidersSub2apiExportMock(...args),
    getProvidersSub2apiExportCandidates: (...args: unknown[]) =>
      getProvidersSub2apiExportCandidatesMock(...args),
  },
}));

describe("useProviderImportExport", () => {
  beforeEach(() => {
    isWebRuntimeMock.mockReset();
    openFileDialogMock.mockReset();
    saveFileDialogMock.mockReset();
    openProvidersFileDialogMock.mockReset();
    saveProvidersFileDialogMock.mockReset();
    importProvidersFromFileMock.mockReset();
    exportProvidersToFileMock.mockReset();
    exportProvidersSub2apiToFileMock.mockReset();
    importProvidersFromContentMock.mockReset();
    downloadProvidersExportMock.mockReset();
    downloadProvidersSub2apiExportMock.mockReset();
    getProvidersSub2apiExportCandidatesMock.mockReset();
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
  });

  it("桌面导出先打开保存路径，再调用 provider-only 导出命令", async () => {
    isWebRuntimeMock.mockReturnValue(false);
    saveProvidersFileDialogMock.mockResolvedValue("/tmp/providers.json");
    exportProvidersToFileMock.mockResolvedValue({
      success: true,
      filePath: "/tmp/providers.json",
    });
    const { result } = renderHook(() => useProviderImportExport());

    await act(async () => {
      await result.current.exportProviders();
    });

    expect(saveProvidersFileDialogMock).toHaveBeenCalledWith(
      expect.stringMatching(/^cc-switch-providers-\d{8}_\d{6}\.json$/),
    );
    expect(saveFileDialogMock).not.toHaveBeenCalled();
    expect(exportProvidersToFileMock).toHaveBeenCalledWith(
      "/tmp/providers.json",
    );
    expect(downloadProvidersExportMock).not.toHaveBeenCalled();
    expect(result.current.status).toBe("success");
  });

  it("桌面导入先打开文件选择，再调用 provider-only 导入命令", async () => {
    isWebRuntimeMock.mockReturnValue(false);
    openProvidersFileDialogMock.mockResolvedValue("/tmp/providers.json");
    importProvidersFromFileMock.mockResolvedValue({ success: true });
    const onImportSuccess = vi.fn();
    const { result } = renderHook(() =>
      useProviderImportExport({ onImportSuccess }),
    );

    await act(async () => {
      await result.current.importProviders();
    });

    expect(openProvidersFileDialogMock).toHaveBeenCalledTimes(1);
    expect(openFileDialogMock).not.toHaveBeenCalled();
    expect(importProvidersFromFileMock).toHaveBeenCalledWith(
      "/tmp/providers.json",
    );
    expect(importProvidersFromContentMock).not.toHaveBeenCalled();
    expect(onImportSuccess).toHaveBeenCalledTimes(1);
    expect(result.current.status).toBe("success");
  });

  it("Web 导出直接调用 providers export 下载接口", async () => {
    isWebRuntimeMock.mockReturnValue(true);
    downloadProvidersExportMock.mockResolvedValue({
      success: true,
      filePath: "providers.json",
    });
    const { result } = renderHook(() => useProviderImportExport());

    await act(async () => {
      await result.current.exportProviders();
    });

    expect(downloadProvidersExportMock).toHaveBeenCalledWith(
      expect.stringMatching(/^cc-switch-providers-\d{8}_\d{6}\.json$/),
    );
    expect(saveProvidersFileDialogMock).not.toHaveBeenCalled();
    expect(saveFileDialogMock).not.toHaveBeenCalled();
    expect(exportProvidersToFileMock).not.toHaveBeenCalled();
    expect(result.current.status).toBe("success");
  });

  it("导出 sub2api 先加载候选账号并打开选择弹窗，默认不选中账号", async () => {
    isWebRuntimeMock.mockReturnValue(false);
    getProvidersSub2apiExportCandidatesMock.mockResolvedValue(
      sub2apiCandidates,
    );
    const { result } = renderHook(() => useProviderImportExport());

    await act(async () => {
      await result.current.exportProvidersSub2api();
    });

    expect(getProvidersSub2apiExportCandidatesMock).toHaveBeenCalledTimes(1);
    expect(result.current.sub2apiExportDialog.open).toBe(true);
    expect(result.current.sub2apiExportDialog.candidates).toEqual(
      sub2apiCandidates,
    );
    expect(result.current.sub2apiExportDialog.selectedProviders).toEqual([]);
    expect(saveProvidersFileDialogMock).not.toHaveBeenCalled();
    expect(exportProvidersSub2apiToFileMock).not.toHaveBeenCalled();
    expect(downloadProvidersSub2apiExportMock).not.toHaveBeenCalled();
    expect(result.current.status).toBe("idle");
  });

  it("桌面导出 sub2api 在确认后把选中账号传给 Tauri 导出命令", async () => {
    isWebRuntimeMock.mockReturnValue(false);
    getProvidersSub2apiExportCandidatesMock.mockResolvedValue(
      sub2apiCandidates,
    );
    saveProvidersFileDialogMock.mockResolvedValue("/tmp/sub2api.json");
    exportProvidersSub2apiToFileMock.mockResolvedValue({
      success: true,
      filePath: "/tmp/sub2api.json",
    });
    const { result } = renderHook(() => useProviderImportExport());

    await act(async () => {
      await result.current.exportProvidersSub2api();
    });
    act(() => {
      result.current.sub2apiExportDialog.toggleProvider(
        { appType: "claude", providerId: "anthropic" },
        true,
      );
    });
    await act(async () => {
      await result.current.sub2apiExportDialog.confirm();
    });

    expect(saveProvidersFileDialogMock).toHaveBeenCalledWith(
      expect.stringMatching(/^sub2api-account-\d{14}\.json$/),
    );
    expect(exportProvidersSub2apiToFileMock).toHaveBeenCalledWith(
      "/tmp/sub2api.json",
      [{ appType: "claude", providerId: "anthropic" }],
    );
    expect(downloadProvidersSub2apiExportMock).not.toHaveBeenCalled();
    expect(exportProvidersToFileMock).not.toHaveBeenCalled();
    expect(result.current.status).toBe("success");
    expect(result.current.sub2apiExportDialog.open).toBe(false);
  });

  it("Web 导出 sub2api 在确认后把选中账号传给下载接口", async () => {
    isWebRuntimeMock.mockReturnValue(true);
    getProvidersSub2apiExportCandidatesMock.mockResolvedValue(
      sub2apiCandidates,
    );
    downloadProvidersSub2apiExportMock.mockResolvedValue({
      success: true,
      filePath: "sub2api-account.json",
    });
    const { result } = renderHook(() => useProviderImportExport());

    await act(async () => {
      await result.current.exportProvidersSub2api();
    });
    act(() => {
      result.current.sub2apiExportDialog.toggleProvider(
        { appType: "codex", providerId: "openrouter" },
        true,
      );
    });
    await act(async () => {
      await result.current.sub2apiExportDialog.confirm();
    });

    expect(downloadProvidersSub2apiExportMock).toHaveBeenCalledWith(
      expect.stringMatching(/^sub2api-account-\d{14}\.json$/),
      [{ appType: "codex", providerId: "openrouter" }],
    );
    expect(saveProvidersFileDialogMock).not.toHaveBeenCalled();
    expect(downloadProvidersExportMock).not.toHaveBeenCalled();
    expect(exportProvidersToFileMock).not.toHaveBeenCalled();
    expect(result.current.status).toBe("success");
  });

  it("取消 sub2api 选择弹窗不会打开保存路径或触发导出", async () => {
    isWebRuntimeMock.mockReturnValue(false);
    getProvidersSub2apiExportCandidatesMock.mockResolvedValue(
      sub2apiCandidates,
    );
    const { result } = renderHook(() => useProviderImportExport());

    await act(async () => {
      await result.current.exportProvidersSub2api();
    });
    act(() => {
      result.current.sub2apiExportDialog.setOpen(false);
    });

    expect(result.current.sub2apiExportDialog.open).toBe(false);
    expect(saveProvidersFileDialogMock).not.toHaveBeenCalled();
    expect(exportProvidersSub2apiToFileMock).not.toHaveBeenCalled();
    expect(downloadProvidersSub2apiExportMock).not.toHaveBeenCalled();
    expect(result.current.status).toBe("idle");
  });

  it("没有选择账号时确认 sub2api 导出不会打开保存路径或触发导出", async () => {
    isWebRuntimeMock.mockReturnValue(false);
    getProvidersSub2apiExportCandidatesMock.mockResolvedValue(
      sub2apiCandidates,
    );
    const { result } = renderHook(() => useProviderImportExport());

    await act(async () => {
      await result.current.exportProvidersSub2api();
    });
    await act(async () => {
      await result.current.sub2apiExportDialog.confirm();
    });

    expect(saveProvidersFileDialogMock).not.toHaveBeenCalled();
    expect(exportProvidersSub2apiToFileMock).not.toHaveBeenCalled();
    expect(downloadProvidersSub2apiExportMock).not.toHaveBeenCalled();
    expect(result.current.status).toBe("idle");
    expect(result.current.sub2apiExportDialog.open).toBe(true);
  });

  it("Web 导入用浏览器 file input 读取内容后提交 providers import 接口", async () => {
    isWebRuntimeMock.mockReturnValue(true);
    importProvidersFromContentMock.mockResolvedValue({ success: true });
    const file = new File(['{"providers":[]}'], "providers.json", {
      type: "application/json",
    });
    Object.defineProperty(file, "text", {
      value: vi.fn().mockResolvedValue('{"providers":[]}'),
      configurable: true,
    });
    const originalCreateElement = document.createElement.bind(document);
    const createElementSpy = vi
      .spyOn(document, "createElement")
      .mockImplementation(((tagName: string) => {
        const element = originalCreateElement(tagName);
        if (tagName.toLowerCase() === "input") {
          queueMicrotask(() => {
            Object.defineProperty(element, "files", {
              value: [file],
              configurable: true,
            });
            element.dispatchEvent(new Event("change"));
          });
        }
        return element;
      }) as typeof document.createElement);
    const { result } = renderHook(() => useProviderImportExport());

    await act(async () => {
      await result.current.importProviders();
    });

    expect(createElementSpy).toHaveBeenCalledWith("input");
    expect(importProvidersFromContentMock).toHaveBeenCalledWith(
      '{"providers":[]}',
    );
    expect(openProvidersFileDialogMock).not.toHaveBeenCalled();
    expect(openFileDialogMock).not.toHaveBeenCalled();
    expect(importProvidersFromFileMock).not.toHaveBeenCalled();
    expect(result.current.status).toBe("success");

    createElementSpy.mockRestore();
  });
});
