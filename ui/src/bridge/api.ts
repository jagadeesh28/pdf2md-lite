import { invoke, isTauri as checkIsTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { AppConfigData, DryRunSummary, ModelStatus, ProgressPayload } from '../types';

export function isTauri(): boolean {
  try {
    return checkIsTauri();
  } catch {
    return false;
  }
}

export function getAuthToken(): string | null {
  if (typeof document === 'undefined') return null;
  const meta = document.querySelector('meta[name="pdfmd-auth-token"]');
  return meta ? meta.getAttribute('content') : null;
}

export function authHeaders(existing?: Record<string, string>): Record<string, string> {
  const headers: Record<string, string> = { ...existing };
  const token = getAuthToken();
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }
  return headers;
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
  output_dir: './output',
};

export function normalizeConfig(raw: Partial<AppConfigData> | null | undefined): AppConfigData {
  if (!raw) return { ...defaultConfig };
  return {
    language: raw.language || defaultConfig.language,
    dom_extraction: raw.dom_extraction || defaultConfig.dom_extraction,
    image_format: raw.image_format || defaultConfig.image_format,
    image_quality:
      typeof raw.image_quality === 'number' && raw.image_quality > 0
        ? raw.image_quality
        : defaultConfig.image_quality,
    embed_images_base64: raw.embed_images_base64 ?? defaultConfig.embed_images_base64,
    extract_tables: raw.extract_tables ?? defaultConfig.extract_tables,
    table_crops: raw.table_crops ?? defaultConfig.table_crops,
    concurrency: raw.concurrency ?? defaultConfig.concurrency,
    embed: raw.embed ?? defaultConfig.embed,
    log_level: raw.log_level || defaultConfig.log_level,
    log_file: raw.log_file ?? defaultConfig.log_file,
    output_dir: raw.output_dir || defaultConfig.output_dir,
  };
}

export async function pickFiles(): Promise<string[]> {
  if (isTauri()) {
    try {
      return await invoke<string[]>('pick_files');
    } catch (err) {
      console.error('Failed to open native file picker:', err);
      return [];
    }
  }
  return [];
}

export async function pickFolder(): Promise<string | null> {
  if (isTauri()) {
    try {
      return await invoke<string | null>('pick_folder');
    } catch (err) {
      console.error('Failed to open native folder picker:', err);
      return null;
    }
  }
  return null;
}

export async function openFolderInExplorer(folderPath: string): Promise<void> {
  if (isTauri()) {
    try {
      await invoke('open_output_folder', { path: folderPath });
    } catch (err) {
      console.error('Failed to open folder in explorer:', err);
    }
  }
}

export async function uploadFile(file: File): Promise<string> {
  const formData = new FormData();
  formData.append('file', file, file.name);
  const res = await fetch('/api/upload', {
    method: 'POST',
    headers: authHeaders(),
    body: formData,
  });
  if (!res.ok) {
    throw new Error(`Upload failed: ${await res.text()}`);
  }
  const data = await res.json();
  return (data.files && data.files[0]) || file.name;
}

export async function fetchConfig(): Promise<AppConfigData> {
  if (isTauri()) {
    try {
      const cfg = await invoke<AppConfigData>('get_config');
      return normalizeConfig(cfg);
    } catch {
      return defaultConfig;
    }
  }

  try {
    const res = await fetch('/api/config', { headers: authHeaders() });
    if (res.ok) {
      const cfg = await res.json();
      return normalizeConfig(cfg);
    }
  } catch {
    // fallback
  }
  return defaultConfig;
}

export async function saveConfig(config: AppConfigData): Promise<void> {
  if (isTauri()) {
    await invoke('save_config', { config });
    return;
  }

  await fetch('/api/config', {
    method: 'PUT',
    headers: authHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify(config),
  });
}

export async function runDryRun(inputPath: string, outputDir: string): Promise<DryRunSummary> {
  if (isTauri()) {
    return await invoke<DryRunSummary>('dry_run', { inputPath, outputDir });
  }

  const res = await fetch('/api/dry-run', {
    method: 'POST',
    headers: authHeaders({ 'Content-Type': 'application/json' }),
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
  if (isTauri()) {
    await invoke('start_conversion', payload);
    return;
  }

  const res = await fetch('/api/convert', {
    method: 'POST',
    headers: authHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify(payload),
  });
  if (!res.ok) {
    throw new Error(`Conversion failed: ${await res.text()}`);
  }
}

export async function formatNotes(vaultPath: string, fixLatex: boolean): Promise<{
  filesProcessed: number;
  replacementsMade: number;
  frontmatterAdded?: number;
  fieldsBackfilled?: number;
}> {
  if (isTauri()) {
    return await invoke('format_notes', { vaultPath, fixLatex });
  }

  const res = await fetch('/api/format-notes', {
    method: 'POST',
    headers: authHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify({ vaultPath, fixLatex }),
  });
  return await res.json();
}

export async function convertPngToJpeg(
  vaultPath: string,
  quality?: number | null
): Promise<{ imagesConverted: number; bytesSaved: number }> {
  const safeQuality =
    typeof quality === 'number' && quality > 0 && quality <= 100
      ? Math.round(quality)
      : 85;

  if (isTauri()) {
    return await invoke('convert_png_to_jpeg', { vaultPath, quality: safeQuality });
  }

  const res = await fetch('/api/convert-png-to-jpeg', {
    method: 'POST',
    headers: authHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify({ vaultPath, quality: safeQuality }),
  });
  if (!res.ok) {
    throw new Error(`Convert PNG to JPEG failed: ${await res.text()}`);
  }
  return await res.json();
}

export async function embedVault(vaultPath: string, force: boolean): Promise<{ notesProcessed: number; chunksEmbedded: number }> {
  if (isTauri()) {
    return await invoke('embed_vault', { vaultPath, force });
  }

  const res = await fetch('/api/embed-vault', {
    method: 'POST',
    headers: authHeaders({ 'Content-Type': 'application/json' }),
    body: JSON.stringify({ vaultPath, force }),
  });
  return await res.json();
}

export async function getModelStatus(): Promise<ModelStatus[]> {
  if (isTauri()) {
    try {
      return await invoke<ModelStatus[]>('get_model_status');
    } catch (err) {
      console.error('Failed to get model status via Tauri IPC:', err);
      return [];
    }
  }

  try {
    const res = await fetch('/api/models', {
      headers: authHeaders(),
    });
    if (!res.ok) {
      throw new Error(`Failed to fetch models: HTTP ${res.status}`);
    }
    return await res.json();
  } catch (err) {
    console.error('Failed to get model status via HTTP:', err);
    return [];
  }
}

export async function downloadModels(): Promise<{ success: boolean; message: string }> {
  if (isTauri()) {
    return await invoke('download_models');
  }

  const res = await fetch('/api/download-models', {
    method: 'POST',
    headers: authHeaders(),
  });
  return await res.json();
}

export function subscribeProgress(onEvent: (payload: ProgressPayload) => void): () => void {
  if (isTauri()) {
    let unlisten: (() => void) | undefined;
    listen<ProgressPayload>('conversion-progress', (e) => {
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
    const token = getAuthToken();
    const wsUrl = `${protocol}//${window.location.host}/api/events${token ? `?token=${encodeURIComponent(token)}` : ''}`;
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

export function onNativeFileDrop(callback: (paths: string[]) => void): () => void {
  if (isTauri()) {
    let unlisten: (() => void) | undefined;
    listen<string[]>('native-file-drop', (e) => {
      if (e.payload) {
        callback(e.payload);
      }
    }).then((unsub) => {
      unlisten = unsub;
    }).catch(console.error);

    return () => {
      if (unlisten) unlisten();
    };
  }
  return () => {};
}

export async function cancelConversion(): Promise<void> {
  if (isTauri()) {
    try {
      await invoke('cancel_conversion');
      return;
    } catch (err) {
      console.warn('Tauri cancel_conversion error:', err);
    }
  }
  try {
    await fetch('/api/cancel', {
      method: 'POST',
      headers: authHeaders(),
    });
  } catch (err) {
    console.warn('Web daemon cancel error:', err);
  }
}

export async function fetchPagePreview(filePath: string, pageNumber: number): Promise<string> {
  if (isTauri()) {
    try {
      const dataUri = await invoke<string>('get_page_preview', {
        filePath,
        page: pageNumber,
      });
      if (dataUri) return dataUri;
    } catch (err) {
      console.warn('Tauri get_page_preview error:', err);
    }
  }
  const token = getAuthToken();
  const tokenParam = token ? `&token=${encodeURIComponent(token)}` : '';
  return `/api/page-preview?slug=${encodeURIComponent(filePath)}&page=${pageNumber}${tokenParam}`;
}

/**
 * Reads a generated Markdown note from disk so the Split-Pane inspector can render
 * the real converted content. Only available in the desktop (Tauri) runtime; the
 * bundled web daemon has no note-read endpoint and falls back to placeholder content.
 */
export async function readMarkdown(path: string): Promise<string> {
  if (isTauri()) {
    return await invoke<string>('read_markdown', { path });
  }
  return '';
}
