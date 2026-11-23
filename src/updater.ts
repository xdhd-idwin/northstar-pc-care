import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

let updateCheck: Promise<void> | undefined;

export function checkForAppUpdate(onProgress: (message: string) => void): Promise<void> {
  if (window.location.protocol === 'http:') return Promise.resolve();
  updateCheck ??= (async () => {
    try {
      const update = await check();
      if (!update) return;
      let received = 0;
      let total = 0;
      onProgress(`Downloading Northstar update ${update.version}…`);
      await update.downloadAndInstall((event) => {
        if (event.event === 'Started') {
          total = event.data.contentLength ?? 0;
        } else if (event.event === 'Progress') {
          received += event.data.chunkLength;
          if (total > 0) {
            onProgress(`Downloading Northstar update ${update.version}… ${Math.min(100, Math.round(received * 100 / total))}%`);
          }
        } else if (event.event === 'Finished') {
          onProgress('Installing Northstar update…');
        }
      });
      await relaunch();
    } catch {
      // Offline or unavailable update feeds should never prevent app startup.
    }
  })();
  return updateCheck;
}
