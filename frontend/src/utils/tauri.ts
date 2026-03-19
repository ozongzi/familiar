/**
 * 最小化的 Tauri invoke 包装。
 */

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const tauri = (window as any).__TAURI_IIFE__ ?? (window as any).__TAURI__;

export const isTauri = (): boolean => !!tauri;

export async function invoke<T = void>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!tauri) {
    return undefined as unknown as T;
  }
  // Tauri 2.x: __TAURI__.core.invoke
  const invoker = tauri?.core?.invoke ?? tauri?.invoke;
  if (!invoker) {
    console.warn("[tauri] invoke 函数未找到");
    return undefined as unknown as T;
  }
  return invoker(cmd, args);
}
