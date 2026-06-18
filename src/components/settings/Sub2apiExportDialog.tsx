import { Download } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import type {
  Sub2apiExportCandidate,
  Sub2apiProviderSelection,
} from "@/lib/api/settings";

interface Sub2apiExportDialogProps {
  open: boolean;
  candidates: Sub2apiExportCandidate[];
  selectedProviders: Sub2apiProviderSelection[];
  isExporting: boolean;
  onOpenChange: (open: boolean) => void;
  onToggleProvider: (
    provider: Sub2apiProviderSelection,
    checked: boolean,
  ) => void;
  onSelectAll: () => void;
  onClearSelection: () => void;
  onConfirm: () => Promise<void>;
}

const selectionKey = (provider: Sub2apiProviderSelection) =>
  `${provider.appType}:${provider.providerId}`;

const candidateSelection = (
  candidate: Sub2apiExportCandidate,
): Sub2apiProviderSelection => ({
  appType: candidate.appType,
  providerId: candidate.providerId,
});

const candidateName = (candidate: Sub2apiExportCandidate) =>
  candidate.name || candidate.providerId;

export function Sub2apiExportDialog({
  open,
  candidates,
  selectedProviders,
  isExporting,
  onOpenChange,
  onToggleProvider,
  onSelectAll,
  onClearSelection,
  onConfirm,
}: Sub2apiExportDialogProps) {
  const { t } = useTranslation();
  const selectedKeys = useMemo(
    () => new Set(selectedProviders.map(selectionKey)),
    [selectedProviders],
  );
  const selectedCount = selectedProviders.length;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[90vh] min-h-0 overflow-hidden">
        <DialogHeader>
          <DialogTitle>{t("settings.sub2apiExportDialog.title")}</DialogTitle>
          <DialogDescription>
            {t("settings.sub2apiExportDialog.description")}
          </DialogDescription>
        </DialogHeader>

        <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-hidden px-6 py-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <Badge variant="secondary">
              {t("settings.sub2apiExportDialog.selectedCount", {
                count: selectedCount,
              })}
            </Badge>
            <div className="flex items-center gap-2">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={onSelectAll}
                disabled={candidates.length === 0 || isExporting}
              >
                {t("settings.sub2apiExportDialog.selectAll")}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={onClearSelection}
                disabled={isExporting}
              >
                {t("settings.sub2apiExportDialog.clear")}
              </Button>
            </div>
          </div>

          <ScrollArea className="h-[min(45dvh,28rem)] min-h-0 rounded-lg border border-border">
            {candidates.length > 0 ? (
              <div className="flex flex-col divide-y divide-border">
                {candidates.map((candidate) => {
                  const selection = candidateSelection(candidate);
                  const checked = selectedKeys.has(selectionKey(selection));
                  const name = candidateName(candidate);
                  return (
                    <label
                      key={selectionKey(selection)}
                      className="flex cursor-pointer items-start gap-3 px-4 py-3 hover:bg-muted/50"
                    >
                      <Checkbox
                        aria-label={name}
                        checked={checked}
                        onCheckedChange={(value) =>
                          onToggleProvider(selection, value === true)
                        }
                        disabled={isExporting}
                      />
                      <span className="min-w-0 flex-1">
                        <span className="flex min-w-0 flex-wrap items-center gap-2">
                          <span className="truncate text-sm font-medium">
                            {name}
                          </span>
                          <Badge variant="outline">{candidate.appType}</Badge>
                        </span>
                        {candidate.baseUrl ? (
                          <span className="mt-1 block truncate text-xs text-muted-foreground">
                            {candidate.baseUrl}
                          </span>
                        ) : null}
                      </span>
                    </label>
                  );
                })}
              </div>
            ) : (
              <div className="px-4 py-10 text-center text-sm text-muted-foreground">
                {t("settings.sub2apiExportDialog.empty")}
              </div>
            )}
          </ScrollArea>
        </div>

        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            onClick={() => onOpenChange(false)}
            disabled={isExporting}
          >
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            onClick={() => void onConfirm()}
            disabled={selectedCount === 0 || isExporting}
          >
            <Download data-icon="inline-start" />
            {t("settings.sub2apiExportDialog.export")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
