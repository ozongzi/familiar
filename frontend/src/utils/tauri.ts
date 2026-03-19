/**
 * 最小化的 Tauri invoke 包装。
 */

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function getTauri(): any {
  return (window as any).__TAURI_IIFE__ ?? (window as any).__TAURI__;
}

export const isTauri = (): boolean => !!getTauri();

export async function invoke<T = void>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const tauri = getTauri();
  if (!tauri) {
    return undefined as unknown as T;
  }
  const invoker = tauri?.core?.invoke ?? tauri?.invoke;
  if (!invoker) {
    console.warn("[tauri] invoke 函数未找到");
    return undefined as unknown as T;
  }
  return invoker(cmd, args);
}

/**
 * 服务器 base URL。
 * - 浏览器环境：空字符串，走相对路径
 * - Tauri 桌面端：从 localStorage 读取，默认 https://familiar.fhmmt.games
 */
export const SERVER_BASE_KEY = "familiar_server";
export const DEFAULT_SERVER = "https://familiar.fhmmt.games";

export function getServerBase(): string {
  if (!isTauri()) return "";
  return localStorage.getItem(SERVER_BASE_KEY) ?? DEFAULT_SERVER;
}
