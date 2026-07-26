import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { Plus, Save, Trash2 } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  settingsApi,
  type RectifierConfig,
  type OptimizerConfig,
  type UserAgentRewriteConfig,
  type UserAgentRewriteRule,
} from "@/lib/api/settings";

const DEFAULT_USER_AGENT_REWRITE_CONFIG: UserAgentRewriteConfig = {
  enabled: true,
  rules: [{ enabled: true, pattern: "^OpenAI/Python\\s+\\d+(?:\\.\\d+)*$" }],
  codexTarget:
    "codex-tui/0.139.0 (Ubuntu 24.4.0; x86_64) unknown (codex-tui; 0.139.0)",
};

const MAX_USER_AGENT_REWRITE_RULES = 32;

function normalizeUserAgentRewriteConfig(
  config: UserAgentRewriteConfig,
): UserAgentRewriteConfig {
  return {
    enabled:
      typeof config.enabled === "boolean"
        ? config.enabled
        : DEFAULT_USER_AGENT_REWRITE_CONFIG.enabled,
    codexTarget:
      typeof config.codexTarget === "string" &&
      config.codexTarget.trim().length > 0
        ? config.codexTarget
        : DEFAULT_USER_AGENT_REWRITE_CONFIG.codexTarget,
    rules: Array.isArray(config.rules)
      ? config.rules.map((rule) => ({
          enabled: rule.enabled ?? true,
          pattern: rule.pattern ?? "",
        }))
      : DEFAULT_USER_AGENT_REWRITE_CONFIG.rules,
  };
}

function validateUserAgentRewriteConfig(
  config: UserAgentRewriteConfig,
): string | null {
  if (!config.codexTarget.trim()) {
    return "settings.advanced.userAgentRewrite.codexTargetRequired";
  }
  if (config.rules.length > MAX_USER_AGENT_REWRITE_RULES) {
    return "settings.advanced.userAgentRewrite.tooManyRules";
  }

  for (const rule of config.rules) {
    if (!rule.enabled) continue;
    if (!rule.pattern.trim()) {
      return "settings.advanced.userAgentRewrite.emptyRule";
    }
    try {
      new RegExp(rule.pattern);
    } catch {
      return "settings.advanced.userAgentRewrite.invalidRegex";
    }
  }

  return null;
}

export function RectifierConfigPanel() {
  const { t } = useTranslation();
  const [config, setConfig] = useState<RectifierConfig>({
    enabled: true,
    requestThinkingSignature: true,
    requestThinkingBudget: true,
    requestMediaFallback: true,
    requestMediaHeuristic: true,
  });
  const [optimizerConfig, setOptimizerConfig] = useState<OptimizerConfig>({
    enabled: false,
    thinkingOptimizer: true,
    cacheInjection: true,
  });
  const [userAgentRewriteConfig, setUserAgentRewriteConfig] =
    useState<UserAgentRewriteConfig>(DEFAULT_USER_AGENT_REWRITE_CONFIG);
  const [savedUserAgentRewriteConfig, setSavedUserAgentRewriteConfig] =
    useState<UserAgentRewriteConfig>(DEFAULT_USER_AGENT_REWRITE_CONFIG);
  const [isSavingUserAgentRewrite, setIsSavingUserAgentRewrite] =
    useState(false);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    settingsApi
      .getRectifierConfig()
      .then(setConfig)
      .catch((e) => console.error("Failed to load rectifier config:", e))
      .finally(() => setIsLoading(false));
    settingsApi
      .getOptimizerConfig()
      .then(setOptimizerConfig)
      .catch((e) => console.error("Failed to load optimizer config:", e));
    settingsApi
      .getUserAgentRewriteConfig()
      .then((nextConfig) => {
        const normalizedConfig = normalizeUserAgentRewriteConfig(nextConfig);
        setUserAgentRewriteConfig(normalizedConfig);
        setSavedUserAgentRewriteConfig(normalizedConfig);
      })
      .catch((e) =>
        console.error("Failed to load User-Agent rewrite config:", e),
      );
  }, []);

  const handleChange = async (updates: Partial<RectifierConfig>) => {
    const newConfig = { ...config, ...updates };
    setConfig(newConfig);
    try {
      await settingsApi.setRectifierConfig(newConfig);
    } catch (e) {
      console.error("Failed to save rectifier config:", e);
      toast.error(String(e));
      setConfig(config);
    }
  };

  const handleOptimizerChange = async (updates: Partial<OptimizerConfig>) => {
    const newConfig = { ...optimizerConfig, ...updates };
    setOptimizerConfig(newConfig);
    try {
      await settingsApi.setOptimizerConfig(newConfig);
    } catch (e) {
      console.error("Failed to save optimizer config:", e);
      toast.error(String(e));
      setOptimizerConfig(optimizerConfig);
    }
  };

  const handleUserAgentRewriteEnabledChange = async (enabled: boolean) => {
    const previousConfig = userAgentRewriteConfig;
    const newDraftConfig = { ...userAgentRewriteConfig, enabled };
    const newSavedConfig = { ...savedUserAgentRewriteConfig, enabled };
    setUserAgentRewriteConfig(newDraftConfig);
    try {
      await settingsApi.setUserAgentRewriteConfig(newSavedConfig);
      setSavedUserAgentRewriteConfig(newSavedConfig);
    } catch (e) {
      console.error("Failed to save User-Agent rewrite config:", e);
      toast.error(String(e));
      setUserAgentRewriteConfig(previousConfig);
    }
  };

  const updateUserAgentRewriteConfig = (
    updates: Partial<UserAgentRewriteConfig>,
  ) => {
    setUserAgentRewriteConfig((current) => ({ ...current, ...updates }));
  };

  const updateUserAgentRewriteRule = (
    index: number,
    updates: Partial<UserAgentRewriteRule>,
  ) => {
    setUserAgentRewriteConfig((current) => ({
      ...current,
      rules: current.rules.map((rule, ruleIndex) =>
        ruleIndex === index ? { ...rule, ...updates } : rule,
      ),
    }));
  };

  const addUserAgentRewriteRule = () => {
    setUserAgentRewriteConfig((current) => {
      if (current.rules.length >= MAX_USER_AGENT_REWRITE_RULES) return current;
      return {
        ...current,
        rules: [...current.rules, { enabled: true, pattern: "" }],
      };
    });
  };

  const removeUserAgentRewriteRule = (index: number) => {
    setUserAgentRewriteConfig((current) => ({
      ...current,
      rules: current.rules.filter((_, ruleIndex) => ruleIndex !== index),
    }));
  };

  const handleUserAgentRewriteSave = async () => {
    const newConfig: UserAgentRewriteConfig = {
      enabled: userAgentRewriteConfig.enabled,
      codexTarget: userAgentRewriteConfig.codexTarget.trim(),
      rules: userAgentRewriteConfig.rules
        .map((rule) => ({
          enabled: rule.enabled ?? true,
          pattern: rule.pattern.trim(),
        }))
        .filter((rule) => rule.pattern.length > 0 || rule.enabled === false),
    };
    const validationKey = validateUserAgentRewriteConfig(newConfig);
    if (validationKey) {
      toast.error(t(validationKey));
      return;
    }

    setIsSavingUserAgentRewrite(true);
    try {
      await settingsApi.setUserAgentRewriteConfig(newConfig);
      setUserAgentRewriteConfig(newConfig);
      setSavedUserAgentRewriteConfig(newConfig);
      toast.success(t("settings.advanced.userAgentRewrite.saved"));
    } catch (e) {
      console.error("Failed to save User-Agent rewrite config:", e);
      toast.error(String(e));
    } finally {
      setIsSavingUserAgentRewrite(false);
    }
  };

  if (isLoading) return null;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="space-y-0.5">
          <Label>{t("settings.advanced.rectifier.enabled")}</Label>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.rectifier.enabledDescription")}
          </p>
        </div>
        <Switch
          checked={config.enabled}
          onCheckedChange={(checked) => handleChange({ enabled: checked })}
        />
      </div>

      <div className="space-y-4">
        <h4 className="text-sm font-medium text-muted-foreground">
          {t("settings.advanced.rectifier.requestGroup")}
        </h4>
        <div className="flex items-center justify-between pl-4">
          <div className="space-y-0.5">
            <Label>{t("settings.advanced.rectifier.thinkingSignature")}</Label>
            <p className="text-xs text-muted-foreground">
              {t("settings.advanced.rectifier.thinkingSignatureDescription")}
            </p>
          </div>
          <Switch
            checked={config.requestThinkingSignature}
            disabled={!config.enabled}
            onCheckedChange={(checked) =>
              handleChange({ requestThinkingSignature: checked })
            }
          />
        </div>
        <div className="flex items-center justify-between pl-4">
          <div className="space-y-0.5">
            <Label>{t("settings.advanced.rectifier.thinkingBudget")}</Label>
            <p className="text-xs text-muted-foreground">
              {t("settings.advanced.rectifier.thinkingBudgetDescription")}
            </p>
          </div>
          <Switch
            checked={config.requestThinkingBudget}
            disabled={!config.enabled}
            onCheckedChange={(checked) =>
              handleChange({ requestThinkingBudget: checked })
            }
          />
        </div>
        <div className="flex items-center justify-between pl-4">
          <div className="space-y-0.5">
            <Label>{t("settings.advanced.rectifier.mediaFallback")}</Label>
            <p className="text-xs text-muted-foreground">
              {t("settings.advanced.rectifier.mediaFallbackDescription")}
            </p>
          </div>
          <Switch
            checked={config.requestMediaFallback}
            disabled={!config.enabled}
            onCheckedChange={(checked) =>
              handleChange({ requestMediaFallback: checked })
            }
          />
        </div>
        <div className="flex items-center justify-between pl-8">
          <div className="space-y-0.5">
            <Label>{t("settings.advanced.rectifier.mediaHeuristic")}</Label>
            <p className="text-xs text-muted-foreground">
              {t("settings.advanced.rectifier.mediaHeuristicDescription")}
            </p>
          </div>
          <Switch
            checked={config.requestMediaHeuristic}
            disabled={!config.enabled || !config.requestMediaFallback}
            onCheckedChange={(checked) =>
              handleChange({ requestMediaHeuristic: checked })
            }
          />
        </div>
      </div>

      <div className="border-t pt-6 mt-6">
        <div className="space-y-1 mb-4">
          <h3 className="text-sm font-medium">
            {t("settings.advanced.userAgentRewrite.title")}
          </h3>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.userAgentRewrite.description")}
          </p>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.userAgentRewrite.saveHint")}
          </p>
        </div>

        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <Label>{t("settings.advanced.userAgentRewrite.enabled")}</Label>
              <p className="text-xs text-muted-foreground">
                {t("settings.advanced.userAgentRewrite.enabledDescription")}
              </p>
            </div>
            <Switch
              checked={userAgentRewriteConfig.enabled}
              aria-label={t("settings.advanced.userAgentRewrite.enabled")}
              onCheckedChange={handleUserAgentRewriteEnabledChange}
            />
          </div>

          <div className="space-y-4 pl-4">
            <div className="space-y-2">
              <Label htmlFor="user-agent-rewrite-codex-target">
                {t("settings.advanced.userAgentRewrite.codexTarget")}
              </Label>
              <Input
                id="user-agent-rewrite-codex-target"
                value={userAgentRewriteConfig.codexTarget}
                disabled={!userAgentRewriteConfig.enabled}
                onChange={(event) =>
                  updateUserAgentRewriteConfig({
                    codexTarget: event.target.value,
                  })
                }
              />
            </div>

            <div className="space-y-2">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <Label>{t("settings.advanced.userAgentRewrite.rules")}</Label>
                  <p className="text-xs text-muted-foreground">
                    {t("settings.advanced.userAgentRewrite.rulesDescription")}
                  </p>
                </div>
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={addUserAgentRewriteRule}
                  disabled={
                    !userAgentRewriteConfig.enabled ||
                    userAgentRewriteConfig.rules.length >=
                      MAX_USER_AGENT_REWRITE_RULES
                  }
                >
                  <Plus className="h-4 w-4" />
                  {t("settings.advanced.userAgentRewrite.addRule")}
                </Button>
              </div>

              <div className="space-y-2">
                {userAgentRewriteConfig.rules.map((rule, index) => (
                  <div
                    key={index}
                    className="grid grid-cols-[auto,minmax(0,1fr),auto] items-center gap-2"
                  >
                    <Switch
                      checked={rule.enabled ?? true}
                      disabled={!userAgentRewriteConfig.enabled}
                      aria-label={t(
                        "settings.advanced.userAgentRewrite.ruleEnabled",
                      )}
                      onCheckedChange={(checked) =>
                        updateUserAgentRewriteRule(index, { enabled: checked })
                      }
                    />
                    <Input
                      value={rule.pattern}
                      disabled={!userAgentRewriteConfig.enabled}
                      onChange={(event) =>
                        updateUserAgentRewriteRule(index, {
                          pattern: event.target.value,
                        })
                      }
                      placeholder={t(
                        "settings.advanced.userAgentRewrite.rulesPlaceholder",
                      )}
                      className="font-mono text-xs"
                    />
                    <Button
                      type="button"
                      size="icon"
                      variant="ghost"
                      aria-label={t(
                        "settings.advanced.userAgentRewrite.deleteRule",
                      )}
                      disabled={!userAgentRewriteConfig.enabled}
                      onClick={() => removeUserAgentRewriteRule(index)}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </div>
                ))}
              </div>
            </div>

            <div className="flex justify-end">
              <Button
                type="button"
                size="sm"
                onClick={handleUserAgentRewriteSave}
                disabled={
                  !userAgentRewriteConfig.enabled || isSavingUserAgentRewrite
                }
              >
                <Save className="h-4 w-4" />
                {isSavingUserAgentRewrite
                  ? t("settings.advanced.userAgentRewrite.saving")
                  : t("settings.advanced.userAgentRewrite.save")}
              </Button>
            </div>
          </div>
        </div>
      </div>

      <div className="border-t pt-6 mt-6">
        <div className="space-y-1 mb-4">
          <h3 className="text-sm font-medium">
            {t("settings.advanced.optimizer.title")}
          </h3>
          <p className="text-xs text-muted-foreground">
            {t("settings.advanced.optimizer.description")}
          </p>
        </div>

        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <div className="space-y-0.5">
              <Label>{t("settings.advanced.optimizer.enabled")}</Label>
            </div>
            <Switch
              checked={optimizerConfig.enabled}
              onCheckedChange={(checked) =>
                handleOptimizerChange({ enabled: checked })
              }
            />
          </div>

          <div className="space-y-4 pl-4">
            <div className="flex items-center justify-between">
              <div className="space-y-0.5">
                <Label>
                  {t("settings.advanced.optimizer.thinkingOptimizer")}
                </Label>
                <p className="text-xs text-muted-foreground">
                  {t(
                    "settings.advanced.optimizer.thinkingOptimizerDescription",
                  )}
                </p>
              </div>
              <Switch
                checked={optimizerConfig.thinkingOptimizer}
                disabled={!optimizerConfig.enabled}
                onCheckedChange={(checked) =>
                  handleOptimizerChange({ thinkingOptimizer: checked })
                }
              />
            </div>

            <div className="flex items-center justify-between">
              <div className="space-y-0.5">
                <Label>{t("settings.advanced.optimizer.cacheInjection")}</Label>
                <p className="text-xs text-muted-foreground">
                  {t("settings.advanced.optimizer.cacheInjectionDescription")}
                </p>
              </div>
              <Switch
                checked={optimizerConfig.cacheInjection}
                disabled={!optimizerConfig.enabled}
                onCheckedChange={(checked) =>
                  handleOptimizerChange({ cacheInjection: checked })
                }
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
