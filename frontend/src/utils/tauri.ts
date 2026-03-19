/**
 * 最小化的 Tauri invoke 包装。
 * 当 @tauri-apps/api 安装后可替换为正式 import。
 */

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const tauri = (window as any).__TAURI_INTERNALS__ ?? (window as any).__TAURI__;

export const isTauri = (): boolean => !!tauri;

export async function invoke<T = void>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!tauri) {
    // 浏览器环境，tunnel 功能不可用，静默忽略
    return undefined as unknown as T;
  }
  return tauri.core
    ? tauri.core.invoke(cmd, args)
    : tauri.invoke(cmd, args);
}
