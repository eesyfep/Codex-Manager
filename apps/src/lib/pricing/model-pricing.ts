import openRouterSnapshot from "./openrouter-model-pricing.snapshot.json";

export interface ModelPricingRate {
  model: string;
  inputPerMillion: number;
  cachedInputPerMillion: number;
  outputPerMillion: number;
  officialLabel: string;
  provider: string;
  sourceUrl: string;
  updatedAt: string;
}

interface OpenRouterSnapshotModel {
  id?: string;
  canonical_slug?: string;
  name?: string;
  pricing?: {
    prompt?: string;
    completion?: string;
    input_cache_read?: string;
  };
}

export const MODEL_PRICING_UPDATED_AT = "2026-05-05";
export const OPENAI_PRICING_SOURCE_URL = "https://openai.com/api/pricing/";
export const OPENROUTER_MODEL_PRICING_SOURCE_URL =
  "https://openrouter.ai/api/v1/models";

const OFFICIAL_MODEL_PRICING: ModelPricingRate[] = [
  {
    model: "gpt-5.5-pro",
    inputPerMillion: 30,
    cachedInputPerMillion: 30,
    outputPerMillion: 180,
    officialLabel: "GPT-5.5 pro",
    provider: "OpenAI",
    sourceUrl: OPENAI_PRICING_SOURCE_URL,
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "gpt-5.5",
    inputPerMillion: 5,
    cachedInputPerMillion: 0.5,
    outputPerMillion: 30,
    officialLabel: "GPT-5.5",
    provider: "OpenAI",
    sourceUrl: OPENAI_PRICING_SOURCE_URL,
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "gpt-5.4-pro",
    inputPerMillion: 30,
    cachedInputPerMillion: 30,
    outputPerMillion: 180,
    officialLabel: "GPT-5.4 pro",
    provider: "OpenAI",
    sourceUrl: OPENAI_PRICING_SOURCE_URL,
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "gpt-5.4-mini",
    inputPerMillion: 0.75,
    cachedInputPerMillion: 0.075,
    outputPerMillion: 4.5,
    officialLabel: "GPT-5.4 mini",
    provider: "OpenAI",
    sourceUrl: OPENAI_PRICING_SOURCE_URL,
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "gpt-5.4-nano",
    inputPerMillion: 0.2,
    cachedInputPerMillion: 0.02,
    outputPerMillion: 1.25,
    officialLabel: "GPT-5.4 nano",
    provider: "OpenAI",
    sourceUrl: OPENAI_PRICING_SOURCE_URL,
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "gpt-5.4",
    inputPerMillion: 2.5,
    cachedInputPerMillion: 0.25,
    outputPerMillion: 15,
    officialLabel: "GPT-5.4",
    provider: "OpenAI",
    sourceUrl: OPENAI_PRICING_SOURCE_URL,
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "gpt-5.3-codex",
    inputPerMillion: 1.75,
    cachedInputPerMillion: 0.175,
    outputPerMillion: 14,
    officialLabel: "GPT-5.3 Codex",
    provider: "OpenAI",
    sourceUrl: OPENAI_PRICING_SOURCE_URL,
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "gpt-5.2",
    inputPerMillion: 1.75,
    cachedInputPerMillion: 0.175,
    outputPerMillion: 14,
    officialLabel: "GPT-5.2",
    provider: "OpenAI",
    sourceUrl: OPENAI_PRICING_SOURCE_URL,
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "gpt-5.1",
    inputPerMillion: 1.25,
    cachedInputPerMillion: 0.125,
    outputPerMillion: 10,
    officialLabel: "GPT-5.1",
    provider: "OpenAI",
    sourceUrl: OPENAI_PRICING_SOURCE_URL,
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "gpt-5",
    inputPerMillion: 1.25,
    cachedInputPerMillion: 0.125,
    outputPerMillion: 10,
    officialLabel: "GPT-5",
    provider: "OpenAI",
    sourceUrl: OPENAI_PRICING_SOURCE_URL,
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "claude-opus-4.5",
    inputPerMillion: 5,
    cachedInputPerMillion: 0.5,
    outputPerMillion: 25,
    officialLabel: "Claude Opus 4.5",
    provider: "Anthropic",
    sourceUrl: "https://www.anthropic.com/pricing",
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "claude-sonnet-4.5",
    inputPerMillion: 3,
    cachedInputPerMillion: 0.3,
    outputPerMillion: 15,
    officialLabel: "Claude Sonnet 4.5",
    provider: "Anthropic",
    sourceUrl: "https://www.anthropic.com/pricing",
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "claude-haiku-4.5",
    inputPerMillion: 1,
    cachedInputPerMillion: 0.1,
    outputPerMillion: 5,
    officialLabel: "Claude Haiku 4.5",
    provider: "Anthropic",
    sourceUrl: "https://www.anthropic.com/pricing",
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "deepseek-chat",
    inputPerMillion: 0.27,
    cachedInputPerMillion: 0.07,
    outputPerMillion: 1.1,
    officialLabel: "DeepSeek Chat",
    provider: "DeepSeek",
    sourceUrl: "https://api-docs.deepseek.com/quick_start/pricing",
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "deepseek-reasoner",
    inputPerMillion: 0.55,
    cachedInputPerMillion: 0.14,
    outputPerMillion: 2.19,
    officialLabel: "DeepSeek Reasoner",
    provider: "DeepSeek",
    sourceUrl: "https://api-docs.deepseek.com/quick_start/pricing",
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "gemini-3-pro",
    inputPerMillion: 2,
    cachedInputPerMillion: 0.2,
    outputPerMillion: 12,
    officialLabel: "Gemini 3 Pro",
    provider: "Google",
    sourceUrl: "https://ai.google.dev/gemini-api/docs/pricing",
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
  {
    model: "gemini-2.5-flash",
    inputPerMillion: 0.3,
    cachedInputPerMillion: 0.03,
    outputPerMillion: 2.5,
    officialLabel: "Gemini 2.5 Flash",
    provider: "Google",
    sourceUrl: "https://ai.google.dev/gemini-api/docs/pricing",
    updatedAt: MODEL_PRICING_UPDATED_AT,
  },
];

const OPENROUTER_MODEL_PRICING: ModelPricingRate[] = (
  (openRouterSnapshot as { data?: OpenRouterSnapshotModel[] }).data ?? []
).map((item) => {
  const prompt = Number(item.pricing?.prompt ?? 0) * 1_000_000;
  const completion = Number(item.pricing?.completion ?? 0) * 1_000_000;
  const cacheRead = Number(item.pricing?.input_cache_read ?? item.pricing?.prompt ?? 0) * 1_000_000;
  const model = item.id || item.canonical_slug || item.name || "unknown";
  return {
    model,
    inputPerMillion: Number.isFinite(prompt) ? prompt : 0,
    cachedInputPerMillion: Number.isFinite(cacheRead) ? cacheRead : 0,
    outputPerMillion: Number.isFinite(completion) ? completion : 0,
    officialLabel: item.name || model,
    provider: model.includes("/") ? model.split("/")[0] : "OpenRouter",
    sourceUrl: OPENROUTER_MODEL_PRICING_SOURCE_URL,
    updatedAt: MODEL_PRICING_UPDATED_AT,
  };
});

export const MODEL_PRICING: ModelPricingRate[] = [
  ...OFFICIAL_MODEL_PRICING,
  ...OPENROUTER_MODEL_PRICING,
];

export function normalizeModelForPricing(model: string | null | undefined): string {
  return (model || "").trim().toLowerCase();
}

export function resolveModelPricing(
  model: string | null | undefined
): ModelPricingRate | null {
  const normalized = normalizeModelForPricing(model);
  if (!normalized) return null;
  return (
    MODEL_PRICING.find((item) => normalized === item.model) ??
    MODEL_PRICING.find((item) => normalized.startsWith(item.model)) ??
    MODEL_PRICING.find((item) => item.model.endsWith(`/${normalized}`)) ??
    null
  );
}

export function formatPricing(rate: ModelPricingRate | null): string {
  if (!rate) return "官价未知";
  return `${rate.provider}: $${rate.inputPerMillion}/$${rate.cachedInputPerMillion}/$${rate.outputPerMillion} per 1M`;
}
