import {
  AlertCircle,
  CheckCircle2,
  Download,
  Loader2,
  Upload,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import type { ProviderTransferStatus } from "@/hooks/useProviderImportExport";

interface ProviderImportExportSectionProps {
  status: ProviderTransferStatus;
  errorMessage: string | null;
  isImporting: boolean;
  isExporting: boolean;
  onImport: () => Promise<void>;
  onExport: () => Promise<void>;
  onExportSub2api: () => Promise<void>;
}

export function ProviderImportExportSection({
  status,
  errorMessage,
  isImporting,
  isExporting,
  onImport,
  onExport,
  onExportSub2api,
}: ProviderImportExportSectionProps) {
  const { t } = useTranslation();
  const busy = isImporting || isExporting;

  return (
    <section className="space-y-4">
      <header className="space-y-2">
        <h3 className="text-base font-semibold text-foreground">
          {t("settings.providerImportExport")}
        </h3>
        <p className="text-sm text-muted-foreground">
          {t("settings.providerImportExportHint")}
        </p>
      </header>

      <div className="space-y-4 rounded-lg border border-border bg-muted/40 p-6">
        <div className="grid grid-cols-1 gap-4 items-stretch sm:grid-cols-3">
          <Button
            type="button"
            className="w-full h-full py-3 px-4 bg-emerald-600 hover:bg-emerald-700 text-white items-center"
            onClick={onImport}
            disabled={busy}
          >
            {isImporting ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <Upload className="mr-2 h-4 w-4" />
            )}
            {isImporting
              ? t("settings.importingProviders")
              : t("settings.importProviders")}
          </Button>

          <Button
            type="button"
            className="w-full h-full py-3 px-4 bg-emerald-600 hover:bg-emerald-700 text-white items-center"
            onClick={onExport}
            disabled={busy}
          >
            {isExporting ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <Download className="mr-2 h-4 w-4" />
            )}
            {isExporting
              ? t("settings.exportingProviders")
              : t("settings.exportProviders")}
          </Button>

          <Button
            type="button"
            className="w-full h-full py-3 px-4 bg-emerald-600 hover:bg-emerald-700 text-white items-center"
            onClick={onExportSub2api}
            disabled={busy}
          >
            <Download className="mr-2 h-4 w-4" />
            {t("settings.exportProvidersSub2api")}
          </Button>
        </div>

        <ProviderTransferStatusMessage
          status={status}
          errorMessage={errorMessage}
        />
      </div>
    </section>
  );
}

interface ProviderTransferStatusMessageProps {
  status: ProviderTransferStatus;
  errorMessage: string | null;
}

function ProviderTransferStatusMessage({
  status,
  errorMessage,
}: ProviderTransferStatusMessageProps) {
  const { t } = useTranslation();

  if (status === "idle") {
    return null;
  }

  const baseClass =
    "flex items-start gap-3 rounded-xl border p-4 text-sm leading-relaxed backdrop-blur-sm";

  if (status === "importing" || status === "exporting") {
    return (
      <div
        className={`${baseClass} border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-400`}
      >
        <Loader2 className="mt-0.5 h-5 w-5 flex-shrink-0 animate-spin" />
        <div>
          <p className="font-semibold">
            {status === "importing"
              ? t("settings.importingProviders")
              : t("settings.exportingProviders")}
          </p>
          <p className="text-emerald-700/80 dark:text-emerald-400/80">
            {t("common.loading")}
          </p>
        </div>
      </div>
    );
  }

  if (status === "success") {
    return (
      <div
        className={`${baseClass} border-green-500/30 bg-green-500/10 text-green-700 dark:text-green-400`}
      >
        <CheckCircle2 className="mt-0.5 h-5 w-5 flex-shrink-0" />
        <div className="space-y-1.5">
          <p className="font-semibold">{t("settings.providerTransferDone")}</p>
          <p className="text-green-600/80 dark:text-green-400/80">
            {t("settings.providerTransferDoneHint")}
          </p>
        </div>
      </div>
    );
  }

  return (
    <div
      className={`${baseClass} border-red-500/30 bg-red-500/10 text-red-600 dark:text-red-400`}
    >
      <AlertCircle className="mt-0.5 h-5 w-5 flex-shrink-0" />
      <div className="space-y-1.5">
        <p className="font-semibold">{t("settings.providerTransferFailed")}</p>
        <p className="text-red-600/80 dark:text-red-400/80">
          {errorMessage || t("settings.providerTransferFailed")}
        </p>
      </div>
    </div>
  );
}
