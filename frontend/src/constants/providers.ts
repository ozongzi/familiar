import type { Provider } from "../api/types";

export const PROVIDER_LABELS: Record<Provider, string> = {
  deepseek:  "DeepSeek",
  openai:    "OpenAI",
  anthropic: "Anthropic",
  gemini:    "Gemini",
  kimi:      "Kimi",
  glm:       "GLM",
  minimax:   "MiniMax",
  grok:      "Grok",
  openrouter: "OpenRouter",
};

export const PROVIDER_DEFAULT_BASE: Record<Provider, string> = {
  deepseek:  "https://api.deepseek.com",
  openai:    "https://api.openai.com/v1",
  anthropic: "https://api.anthropic.com",
  gemini:    "https://generativelanguage.googleapis.com/v1beta",
  kimi:      "https://api.moonshot.cn/v1",
  glm:       "https://open.bigmodel.cn/api/paas/v4",
  minimax:   "https://api.minimaxi.com/anthropic",
  grok:      "https://api.x.ai/v1",
  openrouter: "https://openrouter.ai/api/v1",
};

export const PROVIDERS = Object.keys(PROVIDER_LABELS) as Provider[];
