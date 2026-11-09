import { AppConfigData, DryRunSummary, ProgressPayload } from '../types';

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
    __TAURI__?: {
      core: {
        invoke: <T = unknown>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
      };
      event: {
        listen: <T = unknown>(event: string, handler: (event: { payload: T }) => void) => Promise<() => void>;
      };
    };
  }
}

export function isTauri(): boolean {
  return typeof window !== 'undefined' && Boolean(window.__TAURI_INTERNALS__ || window.__TAURI__);
}

export const defaultConfig: AppConfigData = {
  language: 'en',
  dom_extraction: 'auto',
  image_format: 'jpeg',
  image_quality: 85,
  embed_images_base64: false,
  extract_tables: true,
  table_crops: true,
  concurrency: null,
  embed: false,
  log_level: 'info',
  log_file: null,
};

export async function fetchConfig(): Promise<AppConfigData> {
  if (isTauri() && window.__TAURI__?.core) {
    try {
      return await window.__TAURI__.core.invoke<AppConfigData>('get_config');
    } catch {
      return defaultConfig;
    }
  }

  try {
    const res = await fetch('/api/config');
    if (res.ok) {
      return await res.json();
    }
  } catch {
    // fallback
  }
  return defaultConfig;
}

export async function saveConfig(config: AppConfigData): Promise<void> {
  if (isTauri() && window.__TAURI__?.core) {
    await window.__TAURI__.core.invoke('save_config', { config });
    return;
  }

  await fetch('/api/config', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(config),
  });
}

export async function runDryRun(inputPath: string, outputDir: string): Promise<DryRunSummary> {
  if (isTauri() && window.__TAURI__?.core) {
    return await window.__TAURI__.core.invoke<DryRunSummary>('dry_run', { inputPath, outputDir });
  }

  const res = await fetch('/api/dry-run', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ inputPath, outputDir }),
  });
  if (!res.ok) {
    throw new Error(`Dry run failed: ${await res.text()}`);
  }
  return await res.json();
}

export async function startConversion(payload: {
  files: string[];
  outputDir: string;
  config: AppConfigData;
  resume: boolean;
}): Promise<void> {
  if (isTauri() && window.__TAURI__?.core) {
    await window.__TAURI__.core.invoke('start_conversion', payload);
    return;
  }

  const res = await fetch('/api/convert', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  if (!res.ok) {
    throw new Error(`Conversion failed: ${await res.text()}`);
  }
}

export async function formatNotes(vaultPath: string, fixLatex: boolean): Promise<{ filesProcessed: number; replacementsMade: number }> {
  if (isTauri() && window.__TAURI__?.core) {
    return await window.__TAURI__.core.invoke('format_notes', { vaultPath, fixLatex });
  }

  const res = await fetch('/api/format-notes', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ vaultPath, fixLatex }),
  });
  return await res.json();
}

export async function convertPngToJpeg(vaultPath: string, quality: number): Promise<{ imagesConverted: number; bytesSaved: number }> {
  if (isTauri() && window.__TAURI__?.core) {
    return await window.__TAURI__.core.invoke('convert_png_to_jpeg', { vaultPath, quality });
  }

  const res = await fetch('/api/convert-png-to-jpeg', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ vaultPath, quality }),
  });
  return await res.json();
}

export async function embedVault(vaultPath: string, force: boolean): Promise<{ notesProcessed: number; chunksEmbedded: number }> {
  if (isTauri() && window.__TAURI__?.core) {
    return await window.__TAURI__.core.invoke('embed_vault', { vaultPath, force });
  }

  const res = await fetch('/api/embed-vault', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ vaultPath, force }),
  });
  return await res.json();
}

export async function downloadModels(): Promise<{ success: boolean; message: string }> {
  if (isTauri() && window.__TAURI__?.core) {
    return await window.__TAURI__.core.invoke('download_models');
  }

  const res = await fetch('/api/download-models', { method: 'POST' });
  return await res.json();
}

export function subscribeProgress(onEvent: (payload: ProgressPayload) => void): () => void {
  if (isTauri() && window.__TAURI__?.event) {
    let unlisten: (() => void) | undefined;
    window.__TAURI__.event.listen<ProgressPayload>('conversion-progress', (e) => {
      onEvent(e.payload);
    }).then((unsub) => {
      unlisten = unsub;
    }).catch(console.error);

    return () => {
      if (unlisten) unlisten();
    };
  }

  // Web WebSocket fallback
  let ws: WebSocket | null = null;
  let active = true;

  const connect = () => {
    if (!active) return;
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/api/events`;
    ws = new WebSocket(wsUrl);

    ws.onmessage = (msg) => {
      try {
        const data = JSON.parse(msg.data) as ProgressPayload;
        onEvent(data);
      } catch (err) {
        console.error('Failed to parse WebSocket event payload', err);
      }
    };

    ws.onclose = () => {
      if (active) {
        setTimeout(connect, 2000);
      }
    };
  };

  connect();

  return () => {
    active = false;
    if (ws) {
      ws.close();
    }
  };
}
