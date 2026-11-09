import React, { useState } from 'react';
import {
  FileText,
  Play,
  RotateCcw,
  Eye,
  Trash2,
  Plus,
  FolderPlus,
  CheckCircle2,
  AlertCircle,
  Clock,
  HardDrive,
  Cpu,
} from 'lucide-react';
import { ConversionJob, JobStatus } from '../types';

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
}) => {
  const [resume, setResume] = useState<boolean>(true);
  const [isDragging, setIsDragging] = useState<boolean>(false);

  const completedCount = jobs.filter((j) => j.status === 'completed').length;
  const errorCount = jobs.filter((j) => j.status === 'error').length;
  const runningCount = jobs.filter((j) => j.status === 'running').length;
  const pendingCount = jobs.filter((j) => j.status === 'pending' || j.status === 'idle').length;

  const estimatedMemoryMb = 350 + concurrencyWorkers * 200;

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  };

  const handleDragLeave = () => {
    setIsDragging(false);
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
    const files: string[] = [];
    if (e.dataTransfer.files) {
      for (let i = 0; i < e.dataTransfer.files.length; i++) {
        const file = e.dataTransfer.files[i];
        if (file.name.toLowerCase().endsWith('.pdf')) {
          // In web environment, path might not be full filesystem path,
          // but in Tauri it gives file.path or file.name
          files.push((file as unknown as { path?: string }).path || file.name);
        }
      }
    }
    if (files.length > 0) {
      onAddFiles(files);
    }
  };

  const formatEta = (seconds?: number) => {
    if (seconds === undefined || seconds <= 0) return '--:--';
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}m ${secs.toString().padStart(2, '0')}s`;
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
            onClick={() => {
              const input = document.createElement('input');
              input.type = 'file';
              input.accept = '.pdf';
              input.multiple = true;
              input.onchange = (e) => {
                const target = e.target as HTMLInputElement;
                if (target.files) {
                  const files = Array.from(target.files).map(
                    (f) => (f as unknown as { path?: string }).path || f.name
                  );
                  onAddFiles(files);
                }
              };
              input.click();
            }}
            className="flex items-center space-x-1.5 px-3 py-1.5 bg-gray-700 hover:bg-gray-600 text-white rounded text-xs font-medium transition-colors"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>Add PDF(s)</span>
          </button>

          <button
            onClick={() => {
              // Directory picker
              const input = document.createElement('input');
              input.type = 'file';
              input.webkitdirectory = true;
              input.onchange = (e) => {
                const target = e.target as HTMLInputElement;
                if (target.files) {
                  const files = Array.from(target.files)
                    .filter((f) => f.name.toLowerCase().endsWith('.pdf'))
                    .map((f) => (f as unknown as { path?: string }).path || f.name);
                  onAddFiles(files);
                }
              };
              input.click();
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

      {/* Telemetry Snapshot Banner */}
      <div className="bg-gray-950/70 border-b border-gray-800 px-4 py-2 flex items-center justify-between text-xs text-gray-400">
        <div className="flex items-center space-x-5">
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
          {pagesPerSec !== undefined && (
            <span className="text-purple-300">
              {pagesPerSec.toFixed(1)} pages/sec
            </span>
          )}
          <span className="text-amber-300">
            ETA: {formatEta(currentEta)}
          </span>
        </div>
      </div>

      {/* Jobs Table or Empty Dropzone */}
      <div className="flex-1 overflow-auto p-4">
        {jobs.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center border-2 border-dashed border-gray-700 rounded-xl p-8 text-center text-gray-400">
            <FileText className="w-12 h-12 text-gray-600 mb-3" />
            <h3 className="text-sm font-semibold text-gray-200">No documents in queue</h3>
            <p className="text-xs text-gray-500 max-w-sm mt-1 mb-4">
              Drag and drop PDF files or whole folders here, or use the buttons above to get started.
            </p>
          </div>
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

  return (
    <div className="bg-gray-800/60 hover:bg-gray-800 border border-gray-700/60 rounded-lg p-3 flex items-center justify-between transition-colors">
      <div className="flex items-center space-x-3 overflow-hidden mr-3">
        <FileText className="w-5 h-5 text-purple-400 flex-shrink-0" />
        <div className="overflow-hidden">
          <div className="text-xs font-semibold text-gray-200 truncate">{job.fileName}</div>
          <div className="text-[11px] text-gray-400 truncate">{job.filePath}</div>
        </div>
      </div>

      <div className="flex items-center space-x-3 flex-shrink-0">
        {job.totalPages !== undefined && (
          <span className="text-xs text-gray-400 font-mono">
            {job.completedPages ?? 0} / {job.totalPages} pgs
          </span>
        )}

        {getStatusBadge(job.status)}

        {job.status === 'completed' && (
          <button
            onClick={onInspect}
            className="flex items-center space-x-1 px-2.5 py-1 bg-purple-700/40 hover:bg-purple-700/60 text-purple-200 rounded text-xs transition-colors border border-purple-600/40"
          >
            <Eye className="w-3.5 h-3.5" />
            <span>Inspect</span>
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
  );
};
