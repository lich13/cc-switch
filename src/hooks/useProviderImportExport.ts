import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { settingsApi } from "@/lib/api";
import { isWebRuntime } from "@/lib/runtime";
import type {
  Sub2apiExportCandidate,
  Sub2apiProviderSelection,
} from "@/lib/api/settings";

export type ProviderTransferStatus =
  | "idle"
  | "importing"
  | "exporting"
  | "success"
  | "error";

export interface UseProviderImportExportOptions {
  onImportSuccess?: () => void | Promise<void>;
}

export interface Sub2apiExportDialogState {
  open: boolean;
  candidates: Sub2apiExportCandidate[];
  selectedProviders: Sub2apiProviderSelection[];
  setOpen: (open: boolean) => void;
  toggleProvider: (
    provider: Sub2apiProviderSelection,
    checked: boolean,
  ) => void;
  selectAll: () => void;
  clearSelection: () => void;
  confirm: () => Promise<void>;
}

export interface UseProviderImportExportResult {
  status: ProviderTransferStatus;
  errorMessage: string | null;
  isImporting: boolean;
  isExporting: boolean;
  importProviders: () => Promise<void>;
  exportProviders: () => Promise<void>;
  exportProvidersSub2api: () => Promise<void>;
  sub2apiExportDialog: Sub2apiExportDialogState;
  resetStatus: () => void;
}

const providerExportFileName = () => {
  const now = new Date();
  const stamp = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}${String(now.getDate()).padStart(2, "0")}_${String(now.getHours()).padStart(2, "0")}${String(now.getMinutes()).padStart(2, "0")}${String(now.getSeconds()).padStart(2, "0")}`;
  return `cc-switch-providers-${stamp}.json`;
};

const sub2apiExportFileName = () => {
  const now = new Date();
  const stamp = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}${String(now.getDate()).padStart(2, "0")}${String(now.getHours()).padStart(2, "0")}${String(now.getMinutes()).padStart(2, "0")}${String(now.getSeconds()).padStart(2, "0")}`;
  return `sub2api-account-${stamp}.json`;
};

const providerSelectionKey = (provider: Sub2apiProviderSelection) =>
  `${provider.appType}:${provider.providerId}`;

const toProviderSelection = (
  candidate: Sub2apiExportCandidate,
): Sub2apiProviderSelection => ({
  appType: candidate.appType,
  providerId: candidate.providerId,
});

const pickWebProviderFile = (): Promise<File | null> =>
  new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json,application/json";
    input.style.display = "none";

    const cleanup = () => {
      input.removeEventListener("change", handleChange);
      input.removeEventListener("cancel", handleCancel);
      input.remove();
    };

    const finish = (file: File | null) => {
      cleanup();
      resolve(file);
    };

    const handleChange = () => finish(input.files?.[0] ?? null);
    const handleCancel = () => finish(null);

    input.addEventListener("change", handleChange);
    input.addEventListener("cancel", handleCancel);
    document.body.appendChild(input);
    input.click();
  });

export function useProviderImportExport(
  options: UseProviderImportExportOptions = {},
): UseProviderImportExportResult {
  const { t } = useTranslation();
  const { onImportSuccess } = options;

  const [status, setStatus] = useState<ProviderTransferStatus>("idle");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [sub2apiDialogOpen, setSub2apiDialogOpen] = useState(false);
  const [sub2apiCandidates, setSub2apiCandidates] = useState<
    Sub2apiExportCandidate[]
  >([]);
  const [selectedSub2apiProviders, setSelectedSub2apiProviders] = useState<
    Sub2apiProviderSelection[]
  >([]);

  const resetStatus = useCallback(() => {
    setStatus("idle");
    setErrorMessage(null);
  }, []);

  const setSub2apiExportDialogOpen = useCallback((open: boolean) => {
    setSub2apiDialogOpen(open);
    if (!open) {
      setSelectedSub2apiProviders([]);
    }
  }, []);

  const toggleSub2apiProvider = useCallback(
    (provider: Sub2apiProviderSelection, checked: boolean) => {
      const key = providerSelectionKey(provider);
      setSelectedSub2apiProviders((current) => {
        const exists = current.some(
          (item) => providerSelectionKey(item) === key,
        );
        if (checked) {
          return exists ? current : [...current, provider];
        }
        return current.filter((item) => providerSelectionKey(item) !== key);
      });
    },
    [],
  );

  const selectAllSub2apiProviders = useCallback(() => {
    setSelectedSub2apiProviders(sub2apiCandidates.map(toProviderSelection));
  }, [sub2apiCandidates]);

  const clearSub2apiSelection = useCallback(() => {
    setSelectedSub2apiProviders([]);
  }, []);

  const importProviders = useCallback(async () => {
    if (status === "importing") return;

    setStatus("importing");
    setErrorMessage(null);

    try {
      const result = isWebRuntime()
        ? await (async () => {
            const file = await pickWebProviderFile();
            if (!file) return null;
            return settingsApi.importProvidersFromContent(await file.text());
          })()
        : await (async () => {
            const filePath = await settingsApi.openProvidersFileDialog();
            if (!filePath) return null;
            return settingsApi.importProvidersFromFile(filePath);
          })();

      if (!result) {
        setStatus("idle");
        return;
      }

      if (!result.success) {
        const message =
          result.message ||
          t("settings.providerImportFailed", {
            defaultValue: "导入供应商失败",
          });
        setStatus("error");
        setErrorMessage(message);
        toast.error(message);
        return;
      }

      await onImportSuccess?.();
      setStatus("success");
      toast.success(
        t("settings.providerImportSuccess", {
          defaultValue: "供应商导入成功",
        }),
        { closeButton: true },
      );
    } catch (error) {
      console.error(
        "[useProviderImportExport] Failed to import providers",
        error,
      );
      const message =
        error instanceof Error ? error.message : String(error ?? "");
      setStatus("error");
      setErrorMessage(message);
      toast.error(
        t("settings.providerImportFailedError", {
          defaultValue: "导入供应商失败：{{message}}",
          message,
        }),
      );
    }
  }, [onImportSuccess, status, t]);

  const exportProviders = useCallback(async () => {
    if (status === "exporting") return;

    setStatus("exporting");
    setErrorMessage(null);

    try {
      const defaultName = providerExportFileName();
      const result = isWebRuntime()
        ? await settingsApi.downloadProvidersExport(defaultName)
        : await (async () => {
            const destination =
              await settingsApi.saveProvidersFileDialog(defaultName);
            if (!destination) return null;
            return settingsApi.exportProvidersToFile(destination);
          })();

      if (!result) {
        setStatus("idle");
        return;
      }

      if (!result.success) {
        const message =
          result.message ||
          t("settings.providerExportFailed", {
            defaultValue: "导出供应商失败",
          });
        setStatus("error");
        setErrorMessage(message);
        toast.error(message);
        return;
      }

      setStatus("success");
      toast.success(
        t("settings.providerExportSuccess", {
          defaultValue: "供应商已导出",
        }) + (result.filePath ? `\n${result.filePath}` : ""),
        { closeButton: true },
      );
    } catch (error) {
      console.error(
        "[useProviderImportExport] Failed to export providers",
        error,
      );
      const message =
        error instanceof Error ? error.message : String(error ?? "");
      setStatus("error");
      setErrorMessage(message);
      toast.error(
        t("settings.providerExportFailedError", {
          defaultValue: "导出供应商失败：{{message}}",
          message,
        }),
      );
    }
  }, [status, t]);

  const exportProvidersSub2api = useCallback(async () => {
    if (status === "exporting") return;

    setStatus("exporting");
    setErrorMessage(null);

    try {
      const candidates =
        await settingsApi.getProvidersSub2apiExportCandidates();
      setSub2apiCandidates(candidates);
      setSelectedSub2apiProviders([]);
      setSub2apiDialogOpen(true);
      setStatus("idle");
    } catch (error) {
      console.error(
        "[useProviderImportExport] Failed to load sub2api export candidates",
        error,
      );
      const message =
        error instanceof Error ? error.message : String(error ?? "");
      setStatus("error");
      setErrorMessage(message);
      toast.error(
        t("settings.providerExportFailedError", {
          defaultValue: "导出供应商失败：{{message}}",
          message,
        }),
      );
    }
  }, [status, t]);

  const confirmSub2apiExport = useCallback(async () => {
    if (status === "exporting" || selectedSub2apiProviders.length === 0) return;

    setStatus("exporting");
    setErrorMessage(null);

    try {
      const defaultName = sub2apiExportFileName();
      const result = isWebRuntime()
        ? await settingsApi.downloadProvidersSub2apiExport(
            defaultName,
            selectedSub2apiProviders,
          )
        : await (async () => {
            const destination =
              await settingsApi.saveProvidersFileDialog(defaultName);
            if (!destination) return null;
            return settingsApi.exportProvidersSub2apiToFile(
              destination,
              selectedSub2apiProviders,
            );
          })();

      if (!result) {
        setStatus("idle");
        return;
      }

      if (!result.success) {
        const message =
          result.message ||
          t("settings.providerExportFailed", {
            defaultValue: "导出供应商失败",
          });
        setStatus("error");
        setErrorMessage(message);
        toast.error(message);
        return;
      }

      setStatus("success");
      setSub2apiDialogOpen(false);
      setSelectedSub2apiProviders([]);
      toast.success(
        t("settings.providerExportSuccess", {
          defaultValue: "供应商已导出",
        }) + (result.filePath ? `\n${result.filePath}` : ""),
        { closeButton: true },
      );
    } catch (error) {
      console.error(
        "[useProviderImportExport] Failed to export providers as sub2api",
        error,
      );
      const message =
        error instanceof Error ? error.message : String(error ?? "");
      setStatus("error");
      setErrorMessage(message);
      toast.error(
        t("settings.providerExportFailedError", {
          defaultValue: "导出供应商失败：{{message}}",
          message,
        }),
      );
    }
  }, [selectedSub2apiProviders, status, t]);

  return {
    status,
    errorMessage,
    isImporting: status === "importing",
    isExporting: status === "exporting",
    importProviders,
    exportProviders,
    exportProvidersSub2api,
    sub2apiExportDialog: {
      open: sub2apiDialogOpen,
      candidates: sub2apiCandidates,
      selectedProviders: selectedSub2apiProviders,
      setOpen: setSub2apiExportDialogOpen,
      toggleProvider: toggleSub2apiProvider,
      selectAll: selectAllSub2apiProviders,
      clearSelection: clearSub2apiSelection,
      confirm: confirmSub2apiExport,
    },
    resetStatus,
  };
}
