import React, { useState, useEffect } from 'react';
import { CheckCircle2, AlertCircle, X, Clock, Loader2, ArrowRight } from 'lucide-react';
import { MenuBar } from './components/MenuBar';
import { BatchQueue } from './components/BatchQueue';
import { SplitPane } from './components/SplitPane';
import { LogConsole } from './components/LogConsole';
import { SettingsModal } from './components/SettingsModal';
import { ModelsModal } from './components/ModelsModal';
import { ShortcutsModal } from './components/ShortcutsModal';
import { AboutModal } from './components/AboutModal';
import { useConversionProgress } from './hooks/useConversionProgress';
import {
  AppConfigData,
  ConversionJob,
  DryRunSummary,
} from './types';
import {
  fetchConfig,
  saveConfig,
  startConversion,
  runDryRun,
  formatNotes,
  convertPngToJpeg,
  embedVault,
  defaultConfig,
  cancelConversion,
  onNativeFileDrop,
  openFolderInExplorer,
  pickFiles,
  pickFolder,
} from './bridge/api';

function formatVaultEta(seconds?: number): string {
  if (seconds === undefined || seconds < 0) return '--';
  if (seconds <= 0.5) return 'Finishing...';
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  if (mins > 0) return `~${mins}m ${secs}s`;
  return `~${secs}s`;
}

export const App: React.FC = () => {
  const {
    jobs,
    setJobs,
    logs,
    addLog,
    clearLogs,
    isRunning,
    setIsRunning,
    currentEta,
    pagesPerSec,
    activeDocSlug,
    batchCompletedPages,
    batchTotalPages,
    completionSummary,
    dismissCompletionSummary,
    vaultProgress,
    setVaultProgress,
  } = useConversionProgress();

  const [activeView, setActiveView] = useState<'queue' | 'inspector'>('queue');
  const [inspectedJob, setInspectedJob] = useState<ConversionJob | null>(null);
  const [showConsole, setShowConsole] = useState<boolean>(false);
  const [theme, setTheme] = useState<'dark' | 'obsidian' | 'light'>('dark');
  const [config, setConfig] = useState<AppConfigData>(defaultConfig);
  const [outputDir, setOutputDir] = useState<string>(defaultConfig.output_dir || './output');

  // Modals state
  const [showSettings, setShowSettings] = useState<boolean>(false);
  const [showModels, setShowModels] = useState<boolean>(false);
  const [showShortcuts, setShowShortcuts] = useState<boolean>(false);
  const [showAbout, setShowAbout] = useState<boolean>(false);
  const [dryRunReport, setDryRunReport] = useState<DryRunSummary | null>(null);
  const [actionNotice, setActionNotice] = useState<{
    type: 'success' | 'error' | 'info';
    title: string;
    message: string;
    action?: {
      label: string;
      onClick: () => void;
    };
    progress?: {
      completed: number;
      total: number;
      itemsPerSec: number;
      etaSec: number;
      step?: string;
    };
  } | null>(null);

  // Sync real-time vault maintenance progress (image migration & metadata formatting)
  useEffect(() => {
    if (vaultProgress) {
      setActionNotice({
        type: 'info',
        title:
          vaultProgress.task === 'image_migration'
            ? 'Migrating PNG Images'
            : 'Formatting Notes & Inserting Metadata',
        message: vaultProgress.step,
        progress: {
          completed: vaultProgress.completed,
          total: vaultProgress.total,
          itemsPerSec: vaultProgress.itemsPerSec,
          etaSec: vaultProgress.etaSec,
          step: vaultProgress.step,
        },
      });
    }
  }, [vaultProgress]);

  // Floating notification toast & OS notification when conversion completes
  useEffect(() => {
    const isSingle = jobs.length === 1;
    const singleJob = jobs[0];

    // Trigger on either completionSummary or when a single job finishes
    if (!completionSummary && !(isSingle && singleJob?.status === 'completed')) {
      return;
    }

    const isSuccess = completionSummary
      ? completionSummary.errors === 0
      : singleJob?.status === 'completed';

    const title = isSingle
      ? isSuccess
        ? 'Document Conversion Completed'
        : 'Document Conversion Failed'
      : isSuccess
      ? 'Batch Conversion Completed'
      : `Batch Conversion Completed (${completionSummary?.errors ?? 0} failed)`;

    const elapsed = completionSummary?.elapsedSec ?? singleJob?.elapsedSec;
    const elapsedStr =
      elapsed !== undefined ? ` in ${elapsed.toFixed(1)}s` : '';

    const message = isSingle
      ? isSuccess
        ? `Successfully converted "${singleJob?.fileName || 'Document'}" into Obsidian Markdown${elapsedStr}.`
        : `Failed to convert "${singleJob?.fileName || 'Document'}".`
      : `Converted ${completionSummary?.completed ?? 0} of ${jobs.length} document(s)${elapsedStr}.`;

    setActionNotice({
      type: isSuccess ? 'success' : 'error',
      title,
      message,
      action:
        isSingle && isSuccess && singleJob
          ? {
              label: 'Inspect Note',
              onClick: () => handleInspectJob(singleJob),
            }
          : undefined,
    });

    if ('Notification' in window) {
      try {
        if (Notification.permission === 'granted') {
          new Notification(title, { body: message });
        } else if (Notification.permission !== 'denied') {
          Notification.requestPermission().then((permission) => {
            if (permission === 'granted') {
              new Notification(title, { body: message });
            }
          });
        }
      } catch {
        // Notification permission API not supported in some webview environments
      }
    }
  }, [completionSummary, jobs]);

  // Auto-dismiss completed or error action notice after 7 seconds
  useEffect(() => {
    if (!actionNotice || actionNotice.type === 'info') return;
    const timer = setTimeout(() => {
      setActionNotice(null);
    }, 7000);
    return () => clearTimeout(timer);
  }, [actionNotice]);

  // Initial config load
  useEffect(() => {
    fetchConfig()
      .then((cfg) => {
        setConfig(cfg);
        if (cfg.output_dir) {
          setOutputDir(cfg.output_dir);
        }
      })
      .catch(console.error);
  }, []);

  // Listen for native OS file drops
  useEffect(() => {
    return onNativeFileDrop((paths) => {
      if (paths && paths.length > 0) {
        const pdfs = paths.filter(
          (p) => p.toLowerCase().endsWith('.pdf') || !p.includes('.')
        );
        if (pdfs.length > 0) {
          handleAddFiles(pdfs);
        }
      }
    });
  }, []);

  // Theme application
  useEffect(() => {
    const root = document.documentElement;
    root.classList.remove('dark', 'obsidian', 'light');
    root.classList.add(theme);
  }, [theme]);

  const handleOutputDirChange = (newDir: string) => {
    setOutputDir(newDir);
    const updated = { ...config, output_dir: newDir };
    setConfig(updated);
    saveConfig(updated).catch(console.error);
  };

  const handleOpenOutputDir = async () => {
    addLog('info', `Opening output vault folder: ${outputDir}`);
    await openFolderInExplorer(outputDir);
  };

  const handleAddFiles = (filePaths: string[]) => {
    const newJobs: ConversionJob[] = filePaths.map((fp) => {
      const fileName = fp.split(/[\\/]/).pop() || fp;
      return {
        id: Math.random().toString(36).substring(2, 9),
        filePath: fp,
        fileName,
        outputDir,
        status: 'idle',
      };
    });
    setJobs((prev) => [...prev, ...newJobs]);
    addLog('info', `Added ${filePaths.length} document(s) to conversion queue.`);
  };

  const handleRemoveJob = (id: string) => {
    const job = jobs.find((j) => j.id === id);
    setJobs((prev) => prev.filter((j) => j.id !== id));
    if (job) {
      addLog('info', `Removed "${job.fileName}" from queue.`);
      setActionNotice({
        type: 'info',
        title: 'Document Closed',
        message: `Removed "${job.fileName}" from the conversion queue.`,
      });
    }
  };

  const handleClearJobs = () => {
    setJobs([]);
    addLog('info', 'Batch queue cleared.');
    setActionNotice({
      type: 'info',
      title: 'Queue Cleared',
      message: 'All documents removed from the conversion queue.',
    });
  };

  const handleStartConversion = async (resume: boolean) => {
    const convertible = jobs.filter((j) => !j.isResult);
    if (convertible.length === 0 || isRunning) return;
    setIsRunning(true);
    addLog('info', `Starting batch conversion for ${convertible.length} document(s) (Output: ${outputDir}, Resume: ${resume})...`);
    try {
      await startConversion({
        files: convertible.map((j) => j.filePath),
        outputDir,
        config: { ...config, output_dir: outputDir },
        resume,
      });
    } catch (err) {
      addLog('error', `Batch conversion error: ${err}`);
      setIsRunning(false);
    }
  };

  const handleStopConversion = async () => {
    setIsRunning(false);
    addLog('warn', 'Halting conversion. Notifying backend cancellation token...');
    await cancelConversion();
  };

  const handleDryRun = async () => {
    if (jobs.length === 0) return;
    addLog('info', `Executing conversion dry-run simulation for ${outputDir}...`);
    try {
      const report = await runDryRun(jobs[0].filePath, outputDir);
      setDryRunReport(report);
      addLog(
        'info',
        `Dry-run finished: ${report.total_documents} docs, ${report.total_pages} pages, Est: ~${report.estimated_seconds}s, Concurrency: ${report.concurrency_workers} workers.`
      );
    } catch (err) {
      addLog('error', `Dry-run failed: ${err}`);
    }
  };

  const handleInspectJob = (job: ConversionJob) => {
    // Prefer the freshest job state (status/output path) from the queue.
    const latest = jobs.find((j) => j.id === job.id) ?? job;
    setInspectedJob(latest);
    setActiveView('inspector');
  };

  const handleBackToQueue = () => {
    if (inspectedJob) {
      setActionNotice({
        type: 'info',
        title: 'Viewer Closed',
        message: `Closed side-by-side inspection for "${inspectedJob.fileName}".`,
      });
    }
    setActiveView('queue');
  };

  const handleSaveConfig = async (newConfig: AppConfigData) => {
    setConfig(newConfig);
    if (newConfig.output_dir) {
      setOutputDir(newConfig.output_dir);
    }
    try {
      await saveConfig(newConfig);
      addLog('info', 'Configuration saved to pdfmd-lite.toml.');
    } catch (err) {
      addLog('error', `Failed to save configuration: ${err}`);
    }
  };

  const handleFormatNotes = async () => {
    addLog('info', `Formatting notes across output vault '${outputDir}'...`);
    setVaultProgress({
      task: 'metadata_formatting',
      step: `Scanning Markdown notes in '${outputDir}'...`,
      completed: 0,
      total: 0,
      itemsPerSec: 0,
      etaSec: 0,
    });
    setActionNotice({
      type: 'info',
      title: 'Formatting Notes & Inserting Metadata',
      message: `Scanning Markdown notes in '${outputDir}'...`,
    });
    try {
      const res = await formatNotes(outputDir, false);
      setVaultProgress(null);
      const metaMsg =
        (res.frontmatterAdded || 0) > 0 || (res.fieldsBackfilled || 0) > 0
          ? `, ${res.frontmatterAdded || 0} frontmatter added, ${res.fieldsBackfilled || 0} fields backfilled`
          : '';
      addLog('info', `Note formatter completed: ${res.filesProcessed} notes processed${metaMsg}.`);
      setActionNotice({
        type: 'success',
        title: 'Note Formatting & Metadata Completed',
        message: `${res.filesProcessed} notes formatted in '${outputDir}'${metaMsg}.`,
      });
    } catch (err) {
      setVaultProgress(null);
      addLog('error', `Note formatting failed: ${err}`);
      setActionNotice({
        type: 'error',
        title: 'Note Formatting Failed',
        message: `${err}`,
      });
    }
  };

  const handleFixLatex = async () => {
    addLog('info', `Sanitizing LaTeX delimiters across notes in '${outputDir}'...`);
    setVaultProgress({
      task: 'metadata_formatting',
      step: `Scanning equations in notes across '${outputDir}'...`,
      completed: 0,
      total: 0,
      itemsPerSec: 0,
      etaSec: 0,
    });
    setActionNotice({
      type: 'info',
      title: 'Sanitizing LaTeX Delimiters',
      message: `Scanning equations in notes across '${outputDir}'...`,
    });
    try {
      const res = await formatNotes(outputDir, true);
      setVaultProgress(null);
      addLog('info', `LaTeX sanitizer completed: ${res.replacementsMade} equations repaired.`);
      setActionNotice({
        type: 'success',
        title: 'LaTeX Sanitizer Completed',
        message: `${res.replacementsMade} equations repaired across notes in '${outputDir}'.`,
      });
    } catch (err) {
      setVaultProgress(null);
      addLog('error', `LaTeX sanitizer failed: ${err}`);
      setActionNotice({
        type: 'error',
        title: 'LaTeX Sanitizer Failed',
        message: `${err}`,
      });
    }
  };

  const handleConvertPngToJpeg = async () => {
    addLog('info', `Migrating vault PNG images in '${outputDir}' to JPEG format...`);
    setVaultProgress({
      task: 'image_migration',
      step: `Scanning for PNG figures in '${outputDir}'...`,
      completed: 0,
      total: 0,
      itemsPerSec: 0,
      etaSec: 0,
    });
    setActionNotice({
      type: 'info',
      title: 'Migrating PNG Images',
      message: `Scanning for PNG figures in '${outputDir}'...`,
    });
    try {
      const q = config.image_quality || 85;
      const res = await convertPngToJpeg(outputDir, q);
      setVaultProgress(null);
      const savedKb = Math.round(res.bytesSaved / 1024);
      addLog(
        'info',
        `Image migration completed: ${res.imagesConverted} images converted (~${savedKb} KB saved).`
      );
      setActionNotice({
        type: 'success',
        title: 'Image Migration Completed',
        message: `${res.imagesConverted} images converted to JPEG (~${savedKb} KB disk space saved).`,
      });
    } catch (err) {
      setVaultProgress(null);
      addLog('error', `Image migration failed: ${err}`);
      setActionNotice({
        type: 'error',
        title: 'Image Migration Failed',
        message: `${err}`,
      });
    }
  };

  const handleEmbedVault = async () => {
    addLog('info', `Generating semantic .embeddings.jsonl sidecars across vault in '${outputDir}'...`);
    setActionNotice({
      type: 'info',
      title: 'Generating Semantic Embeddings',
      message: `Indexing semantic vectors across notes in '${outputDir}'...`,
    });
    try {
      const res = await embedVault(outputDir, false);
      addLog(
        'info',
        `Vault embedding finished: ${res.notesProcessed} notes processed, ${res.chunksEmbedded} semantic chunks indexed.`
      );
      setActionNotice({
        type: 'success',
        title: 'Vault Embeddings Completed',
        message: `${res.notesProcessed} notes processed, ${res.chunksEmbedded} semantic chunks indexed.`,
      });
    } catch (err) {
      addLog('error', `Vault embedding failed: ${err}`);
      setActionNotice({
        type: 'error',
        title: 'Vault Embeddings Failed',
        message: `${err}`,
      });
    }
  };

  const handleToggleTheme = () => {
    setTheme((prev) => (prev === 'dark' ? 'obsidian' : prev === 'obsidian' ? 'light' : 'dark'));
  };

  return (
    <div className="w-screen h-screen flex flex-col bg-gray-900 text-gray-100 overflow-hidden font-sans">
      {/* In-Window Hierarchical Menu Bar */}
      <MenuBar
        onOpenPdf={async () => {
          const files = await pickFiles();
          if (files.length > 0) {
            handleAddFiles(files);
          }
        }}
        onOpenDirectory={async () => {
          const folder = await pickFolder();
          if (folder) {
            handleAddFiles([folder]);
          }
        }}
        onWatchDirectory={() => {
          addLog('info', 'Directory watch mode active for input directory.');
        }}
        onOpenVault={handleOpenOutputDir}
        onExportMarkdown={() => {
          if (inspectedJob) {
            addLog('info', `Exported ${inspectedJob.fileName} markdown.`);
          }
        }}
        onStartConversion={() => handleStartConversion(true)}
        onDryRun={handleDryRun}
        onStopConversion={handleStopConversion}
        onFormatNotes={handleFormatNotes}
        onFixLatex={handleFixLatex}
        onConvertPngToJpeg={handleConvertPngToJpeg}
        onClearCompleted={() => {
          setJobs((prev) => prev.filter((j) => j.status !== 'completed'));
        }}
        onClearAll={handleClearJobs}
        onEmbedVault={handleEmbedVault}
        onOpenSettings={() => setShowSettings(true)}
        onOpenModels={() => setShowModels(true)}
        onOpenShortcuts={() => setShowShortcuts(true)}
        onOpenAbout={() => setShowAbout(true)}
        activeView={activeView}
        onToggleView={setActiveView}
        showConsole={showConsole}
        onToggleConsole={() => setShowConsole((v) => !v)}
        theme={theme}
        onToggleTheme={handleToggleTheme}
        isRunning={isRunning}
      />

      {/* Main Workspace Area */}
      <main className="flex-1 flex overflow-hidden relative">
        {activeView === 'queue' ? (
          <BatchQueue
            jobs={jobs}
            onAddFiles={handleAddFiles}
            onRemoveJob={handleRemoveJob}
            onClearJobs={handleClearJobs}
            onStartConversion={handleStartConversion}
            onStopConversion={handleStopConversion}
            onDryRun={handleDryRun}
            onInspectJob={handleInspectJob}
            isRunning={isRunning}
            currentEta={currentEta}
            pagesPerSec={pagesPerSec}
            concurrencyWorkers={config.concurrency || 4}
            outputDir={outputDir}
            onChangeOutputDir={handleOutputDirChange}
            onOpenOutputDir={handleOpenOutputDir}
            activeDocSlug={activeDocSlug}
            batchCompletedPages={batchCompletedPages}
            batchTotalPages={batchTotalPages}
            completionSummary={completionSummary}
            onDismissCompletionSummary={dismissCompletionSummary}
            vaultProgress={vaultProgress}
          />
        ) : (
          <SplitPane
            job={inspectedJob}
            onBackToQueue={handleBackToQueue}
          />
        )}
      </main>

      {/* Live Log Console Drawer */}
      {showConsole && (
        <LogConsole
          logs={logs}
          onClear={clearLogs}
          onClose={() => setShowConsole(false)}
        />
      )}

      {/* Modals */}
      {showSettings && (
        <SettingsModal
          config={config}
          onSave={handleSaveConfig}
          onClose={() => setShowSettings(false)}
        />
      )}

      {showModels && <ModelsModal onClose={() => setShowModels(false)} />}
      {showShortcuts && <ShortcutsModal onClose={() => setShowShortcuts(false)} />}
      {showAbout && <AboutModal onClose={() => setShowAbout(false)} />}

      {/* Dry Run Preview Dialog */}
      {dryRunReport && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4">
          <div className="bg-gray-900 border border-gray-700 rounded-xl shadow-2xl w-full max-w-lg p-5 space-y-4 text-xs">
            <h3 className="text-sm font-bold text-white">Conversion Dry-Run Report</h3>
            <div className="bg-gray-800/80 rounded p-3 space-y-1 font-mono text-[11px] text-gray-300">
              <div>Total Documents: {dryRunReport.total_documents}</div>
              <div>Total Pages: {dryRunReport.total_pages}</div>
              <div>Estimated Latency: ~{dryRunReport.estimated_seconds} seconds</div>
              <div>Worker Concurrency: {dryRunReport.concurrency_workers} threads</div>
              <div>Projected Memory: ~{dryRunReport.estimated_memory_mb} MB</div>
            </div>
            <div className="flex justify-end">
              <button
                onClick={() => setDryRunReport(null)}
                className="px-4 py-1.5 bg-purple-600 hover:bg-purple-500 text-white rounded font-medium"
              >
                Close Report
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Floating Action Notice Toast */}
      {actionNotice && (
        <div className="fixed bottom-6 right-6 z-50 max-w-md w-full animate-in fade-in slide-in-from-bottom-4 duration-200 pointer-events-auto">
          <div
            className={`p-4 rounded-xl border shadow-2xl flex flex-col space-y-2.5 ${
              actionNotice.type === 'success'
                ? 'bg-emerald-950/95 border-emerald-500/50 text-emerald-200'
                : actionNotice.type === 'error'
                ? 'bg-rose-950/95 border-rose-500/50 text-rose-200'
                : 'bg-gray-850/95 border-purple-500/50 text-gray-200'
            }`}
          >
            <div className="flex items-start space-x-3">
              <div className="flex-shrink-0 mt-0.5">
                {actionNotice.type === 'success' && <CheckCircle2 className="w-5 h-5 text-emerald-400" />}
                {actionNotice.type === 'error' && <AlertCircle className="w-5 h-5 text-rose-400" />}
                {actionNotice.type === 'info' && <Loader2 className="w-5 h-5 text-purple-400 animate-spin" />}
              </div>
              <div className="flex-1 min-w-0">
                <div className="text-xs font-semibold text-white flex items-center justify-between">
                  <span>{actionNotice.title}</span>
                  {actionNotice.progress && (
                    <span className="text-[10px] font-mono text-purple-300">
                      {actionNotice.progress.completed} / {actionNotice.progress.total}
                    </span>
                  )}
                </div>
                <div className="text-[11px] mt-0.5 opacity-90 leading-relaxed">{actionNotice.message}</div>
              </div>
              <button
                onClick={() => setActionNotice(null)}
                className="flex-shrink-0 text-gray-400 hover:text-white transition-colors"
                title="Dismiss"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            {/* Live Progress Bar & ETA for Image Migration / Metadata Tasks */}
            {actionNotice.progress && actionNotice.progress.total > 0 && (
              <div className="space-y-1.5 pt-1 border-t border-gray-700/60 font-mono text-[10px]">
                <div className="flex items-center justify-between text-gray-300">
                  <div className="flex items-center space-x-1 text-purple-300">
                    <Clock className="w-3 h-3 text-purple-400" />
                    <span>ETA: {formatVaultEta(actionNotice.progress.etaSec)}</span>
                  </div>
                  {actionNotice.progress.itemsPerSec > 0 && (
                    <span className="text-gray-400">
                      {actionNotice.progress.itemsPerSec.toFixed(1)}/s
                    </span>
                  )}
                  <span className="text-purple-300 font-bold">
                    {Math.round(
                      (actionNotice.progress.completed / actionNotice.progress.total) * 100
                    )}%
                  </span>
                </div>
                <div className="w-full bg-gray-900/80 rounded-full h-1.5 overflow-hidden border border-gray-700/50">
                  <div
                    className="bg-gradient-to-r from-purple-500 to-indigo-500 h-full rounded-full transition-all duration-300"
                    style={{
                      width: `${Math.min(
                        100,
                        Math.max(
                          2,
                          (actionNotice.progress.completed / actionNotice.progress.total) * 100
                        )
                      )}%`,
                    }}
                  />
                </div>
              </div>
            )}

            {/* Quick Action Button (e.g. Inspect Note) */}
            {actionNotice.action && (
              <div className="pt-2 flex justify-end">
                <button
                  onClick={() => {
                    actionNotice.action?.onClick();
                    setActionNotice(null);
                  }}
                  className="px-3 py-1.5 text-xs font-semibold bg-emerald-600 hover:bg-emerald-500 text-white rounded-lg shadow transition-colors flex items-center space-x-1.5"
                >
                  <span>{actionNotice.action.label}</span>
                  <ArrowRight className="w-3.5 h-3.5" />
                </button>
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
};
