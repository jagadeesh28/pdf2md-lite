export type JobStatus = 'idle' | 'pending' | 'running' | 'completed' | 'error' | 'skipped';

export interface ConversionJob {
  id: string;
  filePath: string;
  fileName: string;
  outputDir: string;
  status: JobStatus;
  totalPages?: number;
  completedPages?: number;
  error?: string;
  outputMarkdownPath?: string;
  markdownContent?: string;
  elapsedSec?: number;
  pages?: {
    pageNumber: number;
    markdown: string;
    previewUrl?: string;
  }[];
}

export interface ProgressPayload {
  type: 'doc_start' | 'page_complete' | 'doc_complete' | 'log' | 'batch_summary' | 'vault_progress';
  docSlug?: string;
  workerId?: number;
  page?: number;
  totalPages?: number;
  elapsedSec?: number;
  pagesPerSec?: number;
  etaSec?: number;
  status?: string;
  level?: 'trace' | 'debug' | 'info' | 'warn' | 'error';
  message?: string;
  batchCompletedPages?: number;
  batchTotalPages?: number;
  completed?: number;
  total?: number;
  errors?: number;
  outputDir?: string;
  outputPath?: string;
  task?: string;
  step?: string;
  itemsPerSec?: number;
}

export interface BatchCompletionSummary {
  completed: number;
  errors: number;
  elapsedSec?: number;
  totalPages?: number;
  outputDir?: string;
  finishedAt: string;
}

export interface AppConfigData {
  language: string;
  dom_extraction: 'auto' | 'always' | 'never';
  image_format: 'jpeg' | 'png' | 'webp';
  image_quality: number;
  embed_images_base64: boolean;
  extract_tables: boolean;
  table_crops: boolean;
  concurrency: number | null; // null for auto
  embed: boolean;
  log_level: 'trace' | 'debug' | 'info' | 'warn' | 'error';
  log_file: string | null;
  output_dir?: string | null;
}

export interface DryRunFileReport {
  file_name: string;
  source_path: string;
  target_dir: string;
  page_count: number;
  status: 'WOULD_CONVERT' | 'WOULD_SKIP' | 'WOULD_OVERWRITE';
  skip_reason?: string;
}

export interface DryRunSummary {
  input_path: string;
  output_dir: string;
  total_documents: number;
  total_pages: number;
  estimated_seconds: number;
  concurrency_workers: number;
  estimated_memory_mb: number;
  documents: DryRunFileReport[];
}

export interface LogMessage {
  id: string;
  timestamp: string;
  level: 'trace' | 'debug' | 'info' | 'warn' | 'error';
  message: string;
}

export interface ModelInfo {
  name: string;
  category: 'embeddings' | 'ocr' | 'layout' | 'latex' | 'metadata';
  installed: boolean;
  required: boolean;
  path: string;
  sizeMb: number;
  description: string;
}
