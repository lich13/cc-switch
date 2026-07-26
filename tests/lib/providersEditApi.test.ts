import { beforeEach, describe, expect, it } from "vitest";
import {
  providersApi,
  universalProvidersApi,
  type ProviderEditDetail,
  type UniversalProviderEditDetail,
} from "@/lib/api/providers";
import type { Provider, UniversalProvider } from "@/types";
import {
  resetProviderState,
  setProviderForEdit,
  setUniversalProviderForEdit,
} from "../msw/state";

const providerDetail: Provider = {
  id: "codex-secret",
  name: "Codex Secret",
  category: "custom",
  settingsConfig: {
    auth: { OPENAI_API_KEY: "sk-edit-detail" },
    config: 'model = "gpt-5"',
  },
};

const universalDetail: UniversalProvider = {
  id: "universal-secret",
  name: "Universal Secret",
  providerType: "custom",
  apps: { claude: true, codex: true, gemini: true },
  baseUrl: "https://api.example.com",
  apiKey: "sk-universal-edit-detail",
  models: {},
};

describe("provider edit detail APIs", () => {
  beforeEach(() => {
    resetProviderState();
    setProviderForEdit("codex", providerDetail.id, providerDetail);
    setUniversalProviderForEdit(universalDetail.id, universalDetail);
  });

  it("loads a normal provider edit detail with the app and id contract", async () => {
    const detail: ProviderEditDetail | null = await providersApi.getForEdit(
      providerDetail.id,
      "codex",
    );

    expect(detail).toEqual(providerDetail);
  });

  it("loads a universal provider edit detail by id", async () => {
    const detail: UniversalProviderEditDetail | null =
      await universalProvidersApi.getForEdit(universalDetail.id);

    expect(detail).toEqual(universalDetail);
  });
});
