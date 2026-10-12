// Mirrors the unread To Me count onto the app icon. macOS and Linux take a
// badge count (Linux only on desktops that implement the Unity launcher API,
// such as KDE Plasma and the Ubuntu dock). Tauri does not support badge
// counts on Windows, so there a red dot is overlaid on the taskbar button.

const DOT_SIZE = 16;

function isWindowsPlatform(): boolean {
  return /Windows/i.test(navigator.userAgent);
}

function redDotRgba(): Uint8Array {
  const rgba = new Uint8Array(DOT_SIZE * DOT_SIZE * 4);
  const center = (DOT_SIZE - 1) / 2;
  const radius = DOT_SIZE / 2;
  for (let y = 0; y < DOT_SIZE; y += 1) {
    for (let x = 0; x < DOT_SIZE; x += 1) {
      const distance = Math.hypot(x - center, y - center);
      // One pixel of soft edge so the dot isn't jagged.
      const alpha = Math.max(0, Math.min(1, radius - distance));
      const i = (y * DOT_SIZE + x) * 4;
      rgba[i] = 0xff;
      rgba[i + 1] = 0x3b;
      rgba[i + 2] = 0x30;
      rgba[i + 3] = Math.round(alpha * 255);
    }
  }
  return rgba;
}

export async function setAppBadge(count: number): Promise<void> {
  if (!(globalThis as { isTauri?: boolean }).isTauri) return;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const win = getCurrentWindow();
    if (isWindowsPlatform()) {
      if (count > 0) {
        const { Image } = await import("@tauri-apps/api/image");
        await win.setOverlayIcon(await Image.new(redDotRgba(), DOT_SIZE, DOT_SIZE));
      } else {
        await win.setOverlayIcon(undefined);
      }
      return;
    }
    await win.setBadgeCount(count > 0 ? count : undefined);
  } catch {
    // A badge is a hint; never let it break the app.
  }
}
