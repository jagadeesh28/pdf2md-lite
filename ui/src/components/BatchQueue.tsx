import React, { useState } from 'react';
import {
  FileText,
  Play,
  RotateCcw,
  Eye,
  Trash2,
  Plus,
  Folder,
  FolderPlus,
  ExternalLink,
  CheckCircle2,
  AlertCircle,
  Clock,
  HardDrive,
  Cpu,
  Zap,
  Activity,
  Loader2,
} from 'lucide-react';
import { BatchCompletionSummary, ConversionJob, JobStatus } from '../types';
import { isTauri, pickFiles, pickFolder, uploadFile } from '../bridge/api';

interface BatchQueueProps {
  jobs: ConversionJob[];
  onAddFiles: (files: string[]) => void;
  onRemoveJob: (id: string) => void;
  onClearJobs: () => void;
  onStartConversion: (resume: boolean) => void;
  onStopConversion: () => void;
  onDryRun: () => void;
  onInspectJob: (job: ConversionJob) => void;
  isRunning: boolean;
  currentEta?: number;
  pagesPerSec?: number;
  concurrencyWorkers: number;
  outputDir: string;
  onChangeOutputDir: (newDir: string) => void;
  onOpenOutputDir: () => void;
  activeDocSlug?: string;
  batchCompletedPages?: number;
  batchTotalPages?: number;
  completionSummary?: BatchCompletionSummary | null;
  onDismissCompletionSummary?: () => void;
  vaultProgress?: {
    task: string;
    step: string;
    completed: number;
    total: number;
    itemsPerSec: number;
    etaSec: number;
  } | null;
}

export const BatchQueue: React.FC<BatchQueueProps> = ({
  jobs,
  onAddFiles,
  onRemoveJob,
  onClearJobs,
  onStartConversion,
  onStopConversion,
  onDryRun,
  onInspectJob,
  isRunning,
  currentEta,
  pagesPerSec,
  concurrencyWorkers,
  outputDir,
  onChangeOutputDir,
  onOpenOutputDir,
  activeDocSlug,
  batchCompletedPages,
  batchTotalPages,
  completionSummary,
  onDismissCompletionSummary,
  vaultProgress,
}) => {
  const [resume, setResume] = useState<boolean>(true);
  const [isDragging, setIsDragging] = useState<boolean>(false);

  const completedCount = jobs.filter((j) => j.status === 'completed').length;
  const errorCount = jobs.filter((j) => j.status === 'error').length;
  const runningCount = jobs.filter((j) => j.status === 'running').length;
  const pendingCount = jobs.filter((j) => j.status === 'pending' || j.status === 'idle').length;

  const estimatedMemoryMb = 350 + concurrencyWorkers * 200;

  // Calculate batch pages
  const jobsSumCompletedPages = jobs.reduce((acc, j) => {
    if (j.status === 'completed') return acc + (j.totalPages || 1);
    return acc + (j.completedPages || 0);
  }, 0);

  const jobsSumTotalPages = jobs.reduce((acc, j) => acc + (j.totalPages || 0), 0);

  const effectiveCompletedPages = batchCompletedPages !== undefined && batchCompletedPages > 0
    ? batchCompletedPages
    : jobsSumCompletedPages;

  const effectiveTotalPages = batchTotalPages !== undefined && batchTotalPages > 0
    ? batchTotalPages
    : jobsSumTotalPages;

  const overallPercent = effectiveTotalPages > 0
    ? Math.min(100, Math.round((effectiveCompletedPages / effectiveTotalPages) * 100))
    : jobs.length > 0
    ? Math.min(100, Math.round((completedCount / jobs.length) * 100))
    : 0;

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  };

  const handleDragLeave = () => {
    setIsDragging(false);
  };

  const handleDrop = async (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
    if (!e.dataTransfer.files) return;

    const droppedFiles = Array.from(e.dataTransfer.files).filter((f) =>
      f.name.toLowerCase().endsWith('.pdf')
    );
    if (droppedFiles.length === 0) return;

    if (isTauri()) {
      const paths = droppedFiles
        .map((f) => (f as unknown as { path?: string }).path)
        .filter((p): p is string => Boolean(p));
      if (paths.length > 0) {
        onAddFiles(paths);
      }
    } else {
      const uploadedPaths: string[] = [];
      const errors: string[] = [];
      for (const file of droppedFiles) {
        try {
          const storedPath = await uploadFile(file);
          uploadedPaths.push(storedPath);
        } catch (err) {
          errors.push(`${file.name}: ${err instanceof Error ? err.message : String(err)}`);
        }
      }
      if (errors.length > 0) {
        alert(`Failed to upload ${errors.length} file(s):\n${errors.join('\n')}`);
      }
      if (uploadedPaths.length > 0) {
        onAddFiles(uploadedPaths);
      }
    }
  };

  const formatEta = (seconds?: number) => {
    if (seconds === undefined || !Number.isFinite(seconds)) {
      return isRunning ? 'Calculating...' : '--';
    }
    if (seconds <= 0.5) {
      return isRunning ? 'Finishing...' : '--';
    }
    const totalSecs = Math.round(seconds);
    const hours = Math.floor(totalSecs / 3600);
    const mins = Math.floor((totalSecs % 3600) / 60);
    const secs = Math.floor(totalSecs % 60);
    if (hours > 0) {
      return `~${hours}h ${mins.toString().padStart(2, '0')}m ${secs.toString().padStart(2, '0')}s`;
    }
    return `~${mins}m ${secs.toString().padStart(2, '0')}s`;
  };

  return (
    <div
      className={`flex-1 flex flex-col h-full bg-gray-900 ${
        isDragging ? 'ring-2 ring-purple-500 bg-purple-950/20' : ''
      }`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {/* Top Action Bar */}
      <div className="bg-gray-800/80 border-b border-gray-700/80 px-4 py-2.5 flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center space-x-2">
          <button
            onClick={async () => {
              if (isTauri()) {
                const selected = await pickFiles();
                if (selected && selected.length > 0) {
                  onAddFiles(selected);
                }
              } else {
                const input = document.createElement('input');
                input.type = 'file';
                input.accept = '.pdf';
                input.multiple = true;
                input.onchange = async (e) => {
                  const target = e.target as HTMLInputElement;
                  if (target.files) {
                    const files = Array.from(target.files);
                    const paths: string[] = [];
                    const errors: string[] = [];
                    for (const f of files) {
                      try {
                        const uploaded = await uploadFile(f);
                        paths.push(uploaded);
                      } catch (err) {
                        errors.push(`${f.name}: ${err instanceof Error ? err.message : String(err)}`);
                      }
                    }
                    if (errors.length > 0) {
                      alert(`Upload failed for ${errors.length} file(s):\n${errors.join('\n')}`);
                    }
                    if (paths.length > 0) onAddFiles(paths);
                  }
                };
                input.click();
              }
            }}
            className="flex items-center space-x-1.5 px-3 py-1.5 bg-gray-700 hover:bg-gray-600 text-white rounded text-xs font-medium transition-colors"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>Add PDF(s)</span>
          </button>

          <button
            onClick={async () => {
              if (isTauri()) {
                const selected = await pickFolder();
                if (selected) {
                  onAddFiles([selected]);
                }
              } else {
                const input = document.createElement('input');
                input.type = 'file';
                input.webkitdirectory = true;
                input.onchange = async (e) => {
                  const target = e.target as HTMLInputElement;
                  if (target.files) {
                    const files = Array.from(target.files).filter((f) =>
                      f.name.toLowerCase().endsWith('.pdf')
                    );
                    const uploadedPaths: string[] = [];
                    const errors: string[] = [];
                    for (const f of files) {
                      try {
                        const path = await uploadFile(f);
                        uploadedPaths.push(path);
                      } catch (err) {
                        errors.push(`${f.name}: ${err instanceof Error ? err.message : String(err)}`);
                      }
                    }
                    if (errors.length > 0) {
                      alert(`Folder upload errors:\n${errors.join('\n')}`);
                    }
                    if (uploadedPaths.length > 0) onAddFiles(uploadedPaths);
                  }
                };
                input.click();
              }
            }}
            className="flex items-center space-x-1.5 px-3 py-1.5 bg-gray-700 hover:bg-gray-600 text-white rounded text-xs font-medium transition-colors"
          >
            <FolderPlus className="w-3.5 h-3.5" />
            <span>Add Folder</span>
          </button>

          <div className="h-5 border-r border-gray-600 mx-1" />

          {isRunning ? (
            <button
              onClick={onStopConversion}
              className="flex items-center space-x-1.5 px-3 py-1.5 bg-red-600 hover:bg-red-500 text-white rounded text-xs font-medium shadow-sm transition-colors"
            >
              <RotateCcw className="w-3.5 h-3.5" />
              <span>Stop Conversion</span>
            </button>
          ) : (
            <button
              onClick={() => onStartConversion(resume)}
              disabled={jobs.length === 0}
              className={`flex items-center space-x-1.5 px-3 py-1.5 rounded text-xs font-medium shadow-sm transition-colors ${
                jobs.length === 0
                  ? 'bg-purple-900/50 text-gray-400 cursor-not-allowed'
                  : 'bg-purple-600 hover:bg-purple-500 text-white'
              }`}
            >
              <Play className="w-3.5 h-3.5" />
              <span>Start Conversion</span>
            </button>
          )}

          <button
            onClick={onDryRun}
            disabled={jobs.length === 0 || isRunning}
            className="flex items-center space-x-1.5 px-3 py-1.5 bg-gray-700 hover:bg-gray-600 text-white rounded text-xs font-medium transition-colors disabled:opacity-50"
          >
            <Eye className="w-3.5 h-3.5" />
            <span>Dry-Run</span>
          </button>
        </div>

        <div className="flex items-center space-x-4 text-xs text-gray-300">
          <label className="flex items-center space-x-1.5 cursor-pointer">
            <input
              type="checkbox"
              checked={resume}
              onChange={(e) => setResume(e.target.checked)}
              className="rounded border-gray-600 text-purple-600 focus:ring-purple-500 bg-gray-700"
            />
            <span className="text-gray-300">Resume (-r)</span>
          </label>

          {jobs.length > 0 && (
            <button
              onClick={onClearJobs}
              disabled={isRunning}
              className="text-gray-400 hover:text-red-400 transition-colors p-1"
              title="Clear all jobs"
            >
              <Trash2 className="w-4 h-4" />
            </button>
          )}
        </div>
      </div>

      {/* Output Folder Bar: Clearly shown and modifiable */}
      <div className="bg-gray-850 border-b border-gray-755 px-4 py-2 flex flex-wrap items-center justify-between text-xs gap-3">
        <div className="flex items-center space-x-2 flex-1 min-w-[280px]">
          <span className="text-gray-400 flex items-center space-x-1.5 flex-shrink-0 font-medium">
            <Folder className="w-3.5 h-3.5 text-purple-400" />
            <span>Output Folder:</span>
          </span>
          <input
            type="text"
            value={outputDir}
            onChange={(e) => onChangeOutputDir(e.target.value)}
            placeholder="./output"
            className="flex-1 bg-gray-900 border border-gray-700 hover:border-gray-600 focus:border-purple-500 rounded px-2.5 py-1 text-gray-200 font-mono text-[11px] focus:outline-none transition-colors"
            title="Target directory for converted Markdown notes, cropped figures, and vector sidecars"
          />
          <button
            onClick={async () => {
              if (isTauri()) {
                const selected = await pickFolder();
                if (selected) {
                  onChangeOutputDir(selected);
                }
              }
            }}
            className="flex items-center space-x-1 px-2.5 py-1 bg-gray-750 hover:bg-gray-650 text-white rounded text-xs font-medium border border-gray-700 transition-colors flex-shrink-0"
            title="Select output folder via file dialog"
          >
            <FolderPlus className="w-3.5 h-3.5 text-purple-300" />
            <span>Browse...</span>
          </button>
          <button
            onClick={onOpenOutputDir}
            className="flex items-center space-x-1 px-2.5 py-1 bg-gray-800 hover:bg-gray-700 text-gray-300 hover:text-white rounded text-xs border border-gray-700 transition-colors flex-shrink-0"
            title="Open output folder in File Explorer"
          >
            <ExternalLink className="w-3.5 h-3.5" />
            <span>Open</span>
          </button>
        </div>
      </div>

      {/* Prominent Completion Feedback Banner */}
      {completionSummary && !isRunning && (
        <div className={`mx-4 mt-3 mb-1 p-3.5 rounded-xl border shadow-lg transition-all duration-300 ${
          completionSummary.errors === 0
            ? 'bg-emerald-950/70 border-emerald-600/60 text-emerald-100 shadow-emerald-950/30'
            : 'bg-amber-950/70 border-amber-600/60 text-amber-100 shadow-amber-950/30'
        }`}>
          <div className="flex items-start justify-between gap-3">
            <div className="flex items-start space-x-3">
              <div className={`p-2 rounded-lg ${
                completionSummary.errors === 0 ? 'bg-emerald-500/20 text-emerald-400' : 'bg-amber-500/20 text-amber-400'
              }`}>
                <CheckCircle2 className="w-5 h-5" />
              </div>
              <div className="space-y-1">
                <div className="flex items-center space-x-2">
                  <h4 className="text-sm font-bold tracking-tight">
                    {jobs.length === 1
                      ? 'Conversion Complete — Document Ready!'
                      : `Batch Conversion Complete — ${completionSummary.completed} of ${jobs.length} Processed`}
                  </h4>
                  {completionSummary.errors > 0 && (
                    <span className="text-[11px] px-2 py-0.5 rounded-full font-semibold bg-red-900/60 text-red-300 border border-red-700/60">
                      {completionSummary.errors} error(s)
                    </span>
                  )}
                </div>
                <p className="text-xs text-gray-300">
                  {jobs.length === 1
                    ? `Successfully converted "${jobs[0]?.fileName}" into Obsidian Markdown with native LaTeX math and tables.`
                    : `Converted ${completionSummary.completed} document(s) with full OCR, layout segmentation, and embedding sidecars.`}
                </p>

                {/* Metrics Chips */}
                <div className="flex flex-wrap items-center gap-3 pt-1.5 text-[11px] font-mono">
                  {completionSummary.elapsedSec !== undefined && (
                    <span className="flex items-center space-x-1 px-2 py-0.5 rounded bg-gray-900/70 border border-gray-700/60 text-gray-200">
                      <Clock className="w-3 h-3 text-amber-400" />
                      <span>Duration: {completionSummary.elapsedSec.toFixed(1)}s</span>
                    </span>
                  )}
                  {effectiveCompletedPages > 0 && (
                    <span className="flex items-center space-x-1 px-2 py-0.5 rounded bg-gray-900/70 border border-gray-700/60 text-gray-200">
                      <FileText className="w-3 h-3 text-purple-400" />
                      <span>Pages: {effectiveCompletedPages}</span>
                    </span>
                  )}
                  <span className="flex items-center space-x-1 px-2 py-0.5 rounded bg-gray-900/70 border border-gray-700/60 text-gray-200">
                    <Folder className="w-3 h-3 text-cyan-400" />
                    <span className="truncate max-w-[260px]">Vault: {completionSummary.outputDir || outputDir}</span>
                  </span>
                </div>
              </div>
            </div>

            {/* Quick Action Buttons */}
            <div className="flex items-center space-x-2 flex-shrink-0">
              <button
                onClick={onOpenOutputDir}
                className="flex items-center space-x-1.5 px-3 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg text-xs font-semibold shadow transition-colors"
                title="Open destination vault folder in File Explorer"
              >
                <Folder className="w-3.5 h-3.5" />
                <span>Open Folder</span>
              </button>

              {jobs.length === 1 && jobs[0]?.status === 'completed' && (
                <button
                  onClick={() => onInspectJob(jobs[0])}
                  className="flex items-center space-x-1.5 px-3 py-1.5 bg-purple-600 hover:bg-purple-500 text-white rounded-lg text-xs font-semibold shadow transition-colors"
                  title="Inspect Markdown notes and extracted figures side-by-side"
                >
                  <Eye className="w-3.5 h-3.5" />
                  <span>Inspect Note</span>
                </button>
              )}

              {onDismissCompletionSummary && (
                <button
                  onClick={onDismissCompletionSummary}
                  className="text-gray-400 hover:text-white p-1 rounded transition-colors ml-1"
                  title="Dismiss notification"
                >
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              )}
            </div>
          </div>
        </div>
      )}

      {/* Visual Batch Progress Dashboard */}
      {(isRunning || completedCount > 0 || runningCount > 0) && (
        <div className="bg-gradient-to-r from-gray-950 via-gray-900 to-gray-950 border-b border-gray-800 px-4 py-3 space-y-2.5">
          <div className="flex items-center justify-between">
            <div className="flex items-center space-x-2.5">
              {isRunning ? (
                <div className="flex items-center space-x-2">
                  <span className="relative flex h-3 w-3">
                    <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-purple-400 opacity-75"></span>
                    <span className="relative inline-flex rounded-full h-3 w-3 bg-purple-500"></span>
                  </span>
                  <span className="text-xs font-semibold text-purple-200">
                    {jobs.length === 1
                      ? (activeDocSlug ? `Converting: ${activeDocSlug}` : 'Converting document...')
                      : (activeDocSlug ? `Converting: ${activeDocSlug}` : 'Converting batch...')}
                  </span>
                </div>
              ) : completedCount === jobs.length && jobs.length > 0 ? (
                <div className="flex items-center space-x-2">
                  <CheckCircle2 className="w-4 h-4 text-emerald-400" />
                  <span className="text-xs font-semibold text-emerald-300">
                    {jobs.length === 1 ? 'Document Conversion Completed' : 'Batch Conversion Completed'}
                  </span>
                </div>
              ) : (
                <div className="flex items-center space-x-2">
                  <Activity className="w-4 h-4 text-gray-400" />
                  <span className="text-xs font-semibold text-gray-300">
                    {jobs.length === 1 ? 'Document Conversion Progress' : 'Batch Conversion Progress'}
                  </span>
                </div>
              )}

              {/* Real-time ETA pill */}
              {isRunning && (
                <span className="flex items-center space-x-1 px-2.5 py-0.5 rounded-full text-[11px] font-mono font-semibold bg-amber-500/10 text-amber-300 border border-amber-500/30">
                  <Clock className="w-3 h-3 text-amber-400" />
                  <span>ETA: {formatEta(currentEta)}</span>
                </span>
              )}

              {/* Speed pill */}
              {pagesPerSec !== undefined && pagesPerSec > 0 && (
                <span className="flex items-center space-x-1 px-2.5 py-0.5 rounded-full text-[11px] font-mono font-semibold bg-purple-500/10 text-purple-300 border border-purple-500/30">
                  <Zap className="w-3 h-3 text-purple-400" />
                  <span>{pagesPerSec.toFixed(1)} pages/sec</span>
                </span>
              )}
            </div>

            <div className="flex items-center space-x-3 text-xs font-mono">
              <span className="text-gray-400">
                Pages: <strong className="text-white">{effectiveCompletedPages}</strong> / {effectiveTotalPages > 0 ? effectiveTotalPages : '--'}
              </span>
              <span className="text-gray-600">|</span>
              <span className="text-gray-400">
                Docs: <strong className="text-white">{completedCount}</strong> / {jobs.length}
              </span>
              <span className="text-gray-600">|</span>
              <span className="text-purple-300 font-bold text-sm">
                {overallPercent}%
              </span>
            </div>
          </div>

          {/* Visual Progress Bar */}
          <div className="w-full bg-gray-800/80 rounded-full h-2.5 overflow-hidden p-0.5 border border-gray-700/60 shadow-inner">
            <div
              className={`h-full rounded-full transition-all duration-300 ease-out ${
                isRunning
                  ? 'bg-gradient-to-r from-purple-600 via-indigo-500 to-emerald-400'
                  : completedCount === jobs.length && jobs.length > 0
                  ? 'bg-emerald-500'
                  : 'bg-purple-600'
              }`}
              style={{ width: `${Math.min(100, Math.max(0, overallPercent))}%` }}
            />
          </div>
        </div>
      )}

      {/* Telemetry Snapshot Banner */}
      <div className="bg-gray-950/70 border-b border-gray-800 px-4 py-2 flex items-center justify-between text-xs text-gray-400">
        <div className="flex items-center space-x-5">
          {vaultProgress ? (
            <>
              <span className="flex items-center space-x-1.5 text-purple-300 font-semibold">
                <Loader2 className="w-3.5 h-3.5 animate-spin text-purple-400" />
                <span>
                  {vaultProgress.task === 'image_migration' ? 'Migrating PNGs:' : 'Formatting Notes:'}{' '}
                  <strong className="text-white">{vaultProgress.completed} / {vaultProgress.total}</strong>
                </span>
              </span>
              {vaultProgress.itemsPerSec > 0 && (
                <span className="text-gray-300 font-mono text-[11px]">
                  {vaultProgress.itemsPerSec.toFixed(1)} items/s
                </span>
              )}
            </>
          ) : (
            <>
              <span className="flex items-center space-x-1.5">
                <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400" />
                <span>Completed: <strong className="text-white">{completedCount}</strong></span>
              </span>
              <span className="flex items-center space-x-1.5">
                <Clock className="w-3.5 h-3.5 text-amber-400" />
                <span>Pending: <strong className="text-white">{pendingCount}</strong></span>
              </span>
              {errorCount > 0 && (
                <span className="flex items-center space-x-1.5">
                  <AlertCircle className="w-3.5 h-3.5 text-red-400" />
                  <span>Errors: <strong className="text-white">{errorCount}</strong></span>
                </span>
              )}
              {runningCount > 0 && (
                <span className="flex items-center space-x-1.5">
                  <span className="w-2 h-2 rounded-full bg-purple-500 animate-ping"></span>
                  <span>Running: <strong className="text-white">{runningCount}</strong></span>
                </span>
              )}
            </>
          )}
        </div>

        <div className="flex items-center space-x-5 font-mono text-[11px]">
          <span className="flex items-center space-x-1 text-gray-300">
            <Cpu className="w-3 h-3 text-indigo-400" />
            <span>Workers: {concurrencyWorkers}</span>
          </span>
          <span className="flex items-center space-x-1 text-gray-300">
            <HardDrive className="w-3 h-3 text-cyan-400" />
            <span>Mem Budget: ~{estimatedMemoryMb} MB</span>
          </span>
          {pagesPerSec !== undefined && !vaultProgress && (
            <span className="text-purple-300">
              {pagesPerSec.toFixed(1)} pages/sec
            </span>
          )}
          <span className="text-amber-300 font-semibold">
            ETA: {formatEta(vaultProgress ? vaultProgress.etaSec : currentEta)}
          </span>
        </div>
      </div>

      {/* Jobs Table or Empty Dropzone / Vault Task Dashboard */}
      <div className="flex-1 overflow-auto p-4">
        {jobs.length === 0 ? (
          vaultProgress ? (
            <div className="h-full flex flex-col items-center justify-center p-8 text-center animate-in fade-in duration-200">
              <div className="w-full max-w-xl bg-gray-900/90 border border-purple-500/40 rounded-2xl p-6 shadow-2xl space-y-5">
                <div className="flex items-center space-x-3.5">
                  <div className="p-3 bg-purple-500/20 border border-purple-500/40 rounded-xl text-purple-400">
                    <Loader2 className="w-6 h-6 animate-spin" />
                  </div>
                  <div className="text-left flex-1 min-w-0">
                    <h3 className="text-sm font-bold text-white tracking-tight">
                      {vaultProgress.task === 'image_migration'
                        ? 'Migrating PNG Figures to Compressed JPEG'
                        : 'Formatting Notes & Inserting Metadata'}
                    </h3>
                    <p className="text-xs text-purple-300 font-mono mt-0.5 truncate">
                      {vaultProgress.step}
                    </p>
                  </div>
                  <div className="text-right">
                    <span className="text-xl font-mono font-bold text-purple-400">
                      {vaultProgress.total > 0
                        ? `${Math.round((vaultProgress.completed / vaultProgress.total) * 100)}%`
                        : '0%'}
                    </span>
                  </div>
                </div>

                {/* Large Progress bar */}
                <div className="space-y-1.5">
                  <div className="w-full bg-gray-950 rounded-full h-3 overflow-hidden border border-gray-800">
                    <div
                      className="bg-gradient-to-r from-purple-500 via-indigo-500 to-purple-400 h-full rounded-full transition-all duration-300"
                      style={{
                        width: `${Math.min(
                          100,
                          Math.max(
                            2,
                            vaultProgress.total > 0
                              ? (vaultProgress.completed / vaultProgress.total) * 100
                              : 0
                          )
                        )}%`,
                      }}
                    />
                  </div>
                  <div className="flex justify-between items-center text-[11px] font-mono text-gray-400 pt-1">
                    <span>
                      Progress: <strong className="text-gray-200">{vaultProgress.completed}</strong> / {vaultProgress.total} items
                    </span>
                    {vaultProgress.itemsPerSec > 0 && (
                      <span>Speed: <strong className="text-gray-200">{vaultProgress.itemsPerSec.toFixed(1)}</strong> items/s</span>
                    )}
                    <span className="text-amber-300 font-semibold">
                      ETA: {formatEta(vaultProgress.etaSec)}
                    </span>
                  </div>
                </div>

                <div className="pt-2 border-t border-gray-800 text-[11px] text-gray-400 flex items-center justify-between font-mono">
                  <span className="truncate max-w-[360px]">Target Vault: {outputDir}</span>
                  <span className="text-emerald-400 flex items-center space-x-1">
                    <span className="w-2 h-2 rounded-full bg-emerald-400 animate-pulse"></span>
                    <span>Processing in-place</span>
                  </span>
                </div>
              </div>
            </div>
          ) : (
            <div className="h-full flex flex-col items-center justify-center border-2 border-dashed border-gray-700 rounded-xl p-8 text-center text-gray-400">
              <FileText className="w-12 h-12 text-gray-600 mb-3" />
              <h3 className="text-sm font-semibold text-gray-200">No documents in queue</h3>
              <p className="text-xs text-gray-500 max-w-sm mt-1 mb-4">
                Drag and drop PDF files or whole folders here, or use the buttons above to get started.
              </p>
            </div>
          )
        ) : (
          <div className="space-y-2">
            {jobs.map((job) => (
              <JobItemCard
                key={job.id}
                job={job}
                onRemove={() => onRemoveJob(job.id)}
                onInspect={() => onInspectJob(job)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
};

interface JobItemCardProps {
  job: ConversionJob;
  onRemove: () => void;
  onInspect: () => void;
}

const JobItemCard: React.FC<JobItemCardProps> = ({ job, onRemove, onInspect }) => {
  const getStatusBadge = (status: JobStatus) => {
    switch (status) {
      case 'completed':
        return (
          <span className="px-2 py-0.5 rounded text-[11px] font-medium bg-emerald-950/80 text-emerald-400 border border-emerald-800/60">
            Completed
          </span>
        );
      case 'running':
        return (
          <span className="px-2 py-0.5 rounded text-[11px] font-medium bg-purple-950/80 text-purple-400 border border-purple-800/60 flex items-center space-x-1">
            <span className="w-1.5 h-1.5 rounded-full bg-purple-400 animate-ping"></span>
            <span>Converting</span>
          </span>
        );
      case 'error':
        return (
          <span className="px-2 py-0.5 rounded text-[11px] font-medium bg-red-950/80 text-red-400 border border-red-800/60">
            Error
          </span>
        );
      case 'skipped':
        return (
          <span className="px-2 py-0.5 rounded text-[11px] font-medium bg-gray-800 text-gray-400 border border-gray-700">
            Skipped
          </span>
        );
      default:
        return (
          <span className="px-2 py-0.5 rounded text-[11px] font-medium bg-gray-800 text-gray-300 border border-gray-700">
            Pending
          </span>
        );
    }
  };

  const itemTotalPages = job.totalPages && job.totalPages > 0 ? job.totalPages : 1;
  const itemCompletedPages = job.status === 'completed'
    ? itemTotalPages
    : (job.completedPages ?? 0);
  const itemPercent = Math.min(100, Math.round((itemCompletedPages / itemTotalPages) * 100));

  return (
    <div className={`bg-gray-800/60 hover:bg-gray-800 border rounded-lg p-3 transition-colors ${
      job.status === 'running' ? 'border-purple-600/70 bg-purple-950/10' : 'border-gray-700/60'
    }`}>
      <div className="flex items-center justify-between">
        <div className="flex items-center space-x-3 overflow-hidden mr-3">
          <FileText className={`w-5 h-5 flex-shrink-0 ${job.status === 'running' ? 'text-purple-400 animate-pulse' : 'text-purple-400'}`} />
          <div className="overflow-hidden">
            <div className="text-xs font-semibold text-gray-200 truncate">{job.fileName}</div>
            <div className="text-[11px] text-gray-400 truncate">{job.filePath}</div>
          </div>
        </div>

        <div className="flex items-center space-x-3 flex-shrink-0">
          {job.elapsedSec !== undefined && job.status === 'completed' && (
            <span className="text-[11px] text-emerald-400 font-mono flex items-center space-x-1" title="Conversion duration">
              <Clock className="w-3 h-3 text-emerald-400" />
              <span>{job.elapsedSec.toFixed(1)}s</span>
            </span>
          )}

          {job.totalPages !== undefined && (
            <span className="text-xs text-gray-400 font-mono">
              {job.status === 'completed' ? job.totalPages : (job.completedPages ?? 0)} / {job.totalPages} pgs
            </span>
          )}

          {job.status === 'completed' && (
            <span
              className="px-1.5 py-0.5 rounded text-[10px] font-mono font-medium bg-indigo-950/70 text-indigo-300 border border-indigo-800/50"
              title="Extraction pipeline used"
            >
              {job.pipeline ? job.pipeline.toUpperCase() : 'HYBRID DOM'}
            </span>
          )}

          {job.warnings !== undefined && job.warnings > 0 && (
            <span
              className="px-1.5 py-0.5 rounded text-[10px] font-mono font-medium bg-amber-950/80 text-amber-300 border border-amber-800/60"
              title={`${job.warnings} degradation warning(s)`}
            >
              {job.warnings} {job.warnings === 1 ? 'warning' : 'warnings'}
            </span>
          )}

          {getStatusBadge(job.status)}

          {job.status === 'completed' &&
            (job.isResult || job.outputMarkdownPath || job.markdownContent) && (
            <button
              onClick={onInspect}
              className="flex items-center space-x-1.5 px-3 py-1 bg-purple-600 hover:bg-purple-500 text-white font-semibold rounded text-xs transition-colors shadow-sm"
              title="Inspect Markdown notes and extracted figures side-by-side"
            >
              <Eye className="w-3.5 h-3.5" />
              <span>Inspect Note</span>
            </button>
          )}

          {job.status !== 'running' && (
            <button
              onClick={onRemove}
              className="text-gray-500 hover:text-red-400 p-1 transition-colors"
              title="Remove from queue"
            >
              <Trash2 className="w-4 h-4" />
            </button>
          )}
        </div>
      </div>

      {/* Individual Progress for Converting or In-Progress Job */}
      {(job.status === 'running' || (job.completedPages !== undefined && job.completedPages > 0)) && (
        <div className="mt-2.5 pt-2 border-t border-gray-700/40">
          <div className="flex items-center justify-between text-[11px] font-mono text-gray-400 mb-1">
            <span className="flex items-center space-x-1.5">
              {job.status === 'running' && (
                <span className="w-1.5 h-1.5 rounded-full bg-purple-400 animate-ping inline-block" />
              )}
              <span className={job.status === 'running' ? 'text-purple-300' : 'text-gray-300'}>
                {job.status === 'running' ? 'Processing pages...' : 'Pages converted'}
              </span>
            </span>
            <span className="text-gray-300 font-semibold">
              {itemCompletedPages} / {job.totalPages ?? '?'} pgs ({itemPercent}%)
            </span>
          </div>
          <div className="w-full bg-gray-700/60 rounded-full h-1.5 overflow-hidden">
            <div
              className={`h-full rounded-full transition-all duration-200 ${
                job.status === 'completed'
                  ? 'bg-emerald-500'
                  : job.status === 'error'
                  ? 'bg-red-500'
                  : 'bg-purple-500'
              }`}
              style={{ width: `${Math.min(100, Math.max(0, itemPercent))}%` }}
            />
          </div>
        </div>
      )}

      {job.error && (
        <div className="mt-2 text-[11px] text-red-400 bg-red-950/40 border border-red-900/50 rounded px-2 py-1">
          {job.error}
        </div>
      )}
    </div>
  );
};
