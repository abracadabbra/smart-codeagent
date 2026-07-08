// settings.json 前端类型 — 与 src-tauri/src/settings.rs 保持同步。
//
// Rust 端：
// - Settings / ProviderConfig / McpSettings / ChatMcpServer 均为 #[serde(rename_all = "camelCase")]
// - theme 默认 "dark"
// - provider 字段默认 openai-compatible，缺省时 apiKey 为空字符串

/** LLM Provider 配置。 */
export interface ProviderConfig {
  provider: string;
  apiKey: string;
  baseUrl: string;
  model: string;
  maxTokens: number;
  contextWindowTokens: number;
}

/** settings.json 顶层结构。 */
export interface AppSettings {
  mcp: {
    servers: import("./mcp").ChatMcpServer[];
  };
  theme: "dark" | "light";
  provider: ProviderConfig;
}

/** ProviderConfig 默认值（与 Rust Default 对齐）。 */
export function defaultProviderConfig(): ProviderConfig {
  return {
    provider: "openai-compatible",
    apiKey: "",
    baseUrl: "https://token.sensenova.cn",
    model: "deepseek-v4-flash",
    maxTokens: 8192,
    contextWindowTokens: 56000,
  };
}
