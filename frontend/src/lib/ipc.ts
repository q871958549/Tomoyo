// Typed WebView2 bridge, theme synchronization, and global error telemetry.

declare global {
  interface Window {
    chrome?: {
      webview?: {
        postMessage(msg: string): void;
        addEventListener(type: 'message', listener: (ev: { data: unknown }) => void): void;
        removeEventListener?(type: 'message', listener: (ev: { data: unknown }) => void): void;
      };
    };
    __TOMOYO_SETTINGS_INIT__?: unknown;
    __TOMOYO_TR_INIT__?: unknown;
    __TOMOYO_CLIP_INIT__?: unknown;
    __TOMOYO_OCR_DATA__?: unknown;
    tomoyo?: {
      version?: string;
      platform?: string;
      pluginId?: string;
      initialQuery?: string;
      theme?: string;
      isDarkColors?: () => boolean;
      callPlugin?: <T = unknown>(method: string, params?: unknown) => Promise<T>;
      onEnter?: (cb: (action: { code?: string; type?: string; payload?: string }) => void) => void;
    };
  }
}

export function postHost(msg: Record<string, unknown>): void {
  try {
    window.chrome?.webview?.postMessage(JSON.stringify(msg));
  } catch {
    // Running outside WebView2 (e.g. unit tests)
  }
}

export function subscribeHost(handler: (data: Record<string, unknown>) => void): () => void {
  const wv = window.chrome?.webview;
  if (!wv) return () => {};
  const listener = (ev: { data: unknown }) => {
    let data = ev.data;
    if (typeof data === 'string') {
      try {
        data = JSON.parse(data);
      } catch {
        return;
      }
    }
    if (data && typeof data === 'object' && 'event' in data) {
      handler(data as Record<string, unknown>);
    }
  };
  wv.addEventListener('message', listener);
  return () => {
    wv.removeEventListener?.('message', listener);
  };
}

export function installErrorTelemetry(source: string): void {
  window.addEventListener('error', (e) => {
    postHost({
      cmd: 'report_js_error',
      message: String(e.message || 'error'),
      source,
      line: e.lineno || 0,
    });
  });
  window.addEventListener('unhandledrejection', (e) => {
    postHost({
      cmd: 'report_js_error',
      message: String(e.reason || 'unhandledrejection'),
      source,
      line: 0,
    });
  });
}

export function applyThemeClass(theme: string): void {
  const norm = (theme || 'dark').toLowerCase();
  document.body.classList.remove('theme-dark', 'theme-midnight');
  if (norm === 'dark') document.body.classList.add('theme-dark');
  else if (norm === 'midnight') document.body.classList.add('theme-midnight');
  document.documentElement.style.colorScheme = norm === 'light' ? 'light' : 'dark';
}
