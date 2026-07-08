import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

/** 下载进度事件。 */
export type DownloadEvent =
  | { kind: "started"; contentLength: number | null }
  | { kind: "progress"; chunkLength: number; downloaded: number; contentLength: number | null }
  | { kind: "finished" };

/** 更新信息（简化版，统一字段名）。 */
export interface UpdateInfo {
  /** 当前运行版本。 */
  currentVersion: string;
  /** 最新版本。 */
  version: string;
  /** 更新说明。 */
  body: string;
  /** 发布日期（ISO 字符串）。 */
  date?: string | null;
}

/**
 * 检查更新。
 * @returns 有新版本返回更新信息，否则返回 null。
 */
export async function checkUpdate(): Promise<UpdateInfo | null> {
  const update = await check();
  if (!update) return null;
  return {
    currentVersion: update.currentVersion,
    version: update.version,
    body: update.body ?? "",
    date: update.date ?? null,
  };
}

/**
 * 下载并安装更新，完成后自动重启。
 * @param onProgress 下载进度回调。
 */
export async function downloadAndInstall(
  onProgress?: (event: DownloadEvent) => void
): Promise<void> {
  const update = await check();
  if (!update) return;

  let downloaded = 0;
  let contentLength: number | null = null;

  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        contentLength = event.data.contentLength ?? null;
        onProgress?.({
          kind: "started",
          contentLength,
        });
        break;
      case "Progress":
        downloaded += event.data.chunkLength;
        onProgress?.({
          kind: "progress",
          chunkLength: event.data.chunkLength,
          downloaded,
          contentLength,
        });
        break;
      case "Finished":
        onProgress?.({ kind: "finished" });
        break;
    }
  });

  await relaunch();
}
