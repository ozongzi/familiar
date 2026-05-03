import { isTauri } from "./tauri";

/**
 * Run once on app boot inside the Tauri shell. Checks the configured
 * updater endpoint, downloads + installs any update, and relaunches.
 * Silent on web / Android (Android updates aren't wired through the plugin).
 */
export async function checkForAppUpdate(): Promise<void> {
  if (!isTauri()) return;

  // Lazy import — these modules don't exist in the web bundle's runtime,
  // but Vite still needs them resolvable.
  let check: typeof import("@tauri-apps/plugin-updater").check;
  let relaunch: typeof import("@tauri-apps/plugin-process").relaunch;
  try {
    ({ check } = await import("@tauri-apps/plugin-updater"));
    ({ relaunch } = await import("@tauri-apps/plugin-process"));
  } catch {
    // Plugin not registered (e.g. Android build) — bail quietly.
    return;
  }

  try {
    const update = await check();
    if (!update) return;

    // No UI gate — minor user-facing toast could replace this later.
    console.info(
      `[update] new version ${update.version} available, downloading…`,
    );
    await update.downloadAndInstall();
    await relaunch();
  } catch (err) {
    console.warn("[update] check failed:", err);
  }
}
