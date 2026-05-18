import { Copy, Loader2, Radio } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  useCodexLocalRouteInfo,
  useProxyRoutingMode,
  useSetProxyRoutingModeForApp,
} from "@/lib/query/proxy";
import { copyText } from "@/lib/clipboard";
import { cn } from "@/lib/utils";

interface CodexRoutingControlsProps {
  className?: string;
}

export function CodexRoutingControls({ className }: CodexRoutingControlsProps) {
  const { t } = useTranslation();
  const { data: routingMode } = useProxyRoutingMode("codex");
  const { data: routeInfo } = useCodexLocalRouteInfo();
  const setRoutingMode = useSetProxyRoutingModeForApp();

  const enabled = routingMode === "local_only" || routeInfo?.enabled === true;
  const isBusy = setRoutingMode.isPending;

  const handleToggle = async (checked: boolean) => {
    try {
      await setRoutingMode.mutateAsync({
        appType: "codex",
        mode: checked ? "local_only" : "off",
      });
    } catch {
      // The mutation hook already reports the failure through a toast.
    }
  };

  const handleCopy = async () => {
    if (!routeInfo?.launchCommand) return;
    try {
      await copyText(routeInfo.launchCommand);
      toast.success(
        t("proxy.codexRoute.commandCopied", {
          defaultValue: "Codex launch command copied",
        }),
        { closeButton: true },
      );
    } catch (error) {
      console.error("[CodexRoutingControls] copy launch command failed", error);
    }
  };

  const tooltipText = enabled
    ? t("proxy.codexRoute.tooltip.active", {
        provider: routeInfo?.activeProviderName ?? "-",
        baseUrl: routeInfo?.baseUrl ?? "http://127.0.0.1:15721/v1",
        defaultValue: `Codex pure route active. Current target: ${routeInfo?.activeProviderName ?? "-"} - ${routeInfo?.baseUrl ?? "http://127.0.0.1:15721/v1"}`,
      })
    : t("proxy.codexRoute.tooltip.inactive", {
        defaultValue:
          "Start Codex pure local route without modifying auth.json or config.toml",
      });

  return (
    <div
      className={cn(
        "flex items-center gap-1 px-1.5 h-8 rounded-lg bg-muted/50 transition-all",
        className,
      )}
      title={tooltipText}
    >
      {isBusy ? (
        <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
      ) : (
        <Radio
          className={cn(
            "h-4 w-4 transition-colors",
            enabled ? "text-sky-500 animate-pulse" : "text-muted-foreground",
          )}
        />
      )}
      <Switch
        checked={enabled}
        onCheckedChange={handleToggle}
        disabled={isBusy}
      />
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="h-7 w-7"
        onClick={handleCopy}
        disabled={!routeInfo?.launchCommand}
        title={t("proxy.codexRoute.copyCommand", {
          defaultValue: "Copy Codex launch command",
        })}
      >
        <Copy className="h-3.5 w-3.5" />
      </Button>
    </div>
  );
}
