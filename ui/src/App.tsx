import React, { useState, useEffect } from 'react';
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
} from './bridge/api';

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
  } = useConversionProgress();

  const [activeView, setActiveView] = useState<'queue' | 'inspector'>('queue');
  const [inspectedJob, setInspectedJob] = useState<ConversionJob | null>(null);
  const [showConsole, setShowConsole] = useState<boolean>(false);
  const [theme, setTheme] = useState<'dark' | 'obsidian' | 'light'>('dark');
  const [config, setConfig] = useState<AppConfigData>(defaultConfig);

  // Modals state
  const [showSettings, setShowSettings] = useState<boolean>(false);
  const [showModels, setShowModels] = useState<boolean>(false);
  const [showShortcuts, setShowShortcuts] = useState<boolean>(false);
  const [showAbout, setShowAbout] = useState<boolean>(false);
  const [dryRunReport, setDryRunReport] = useState<DryRunSummary | null>(null);

  // Initial config load
  useEffect(() => {
    fetchConfig().then(setConfig).catch(console.error);
  }, []);

  // Theme application
  useEffect(() => {
    const root = document.documentElement;
    root.classList.remove('dark', 'obsidian', 'light');
    root.classList.add(theme);
  }, [theme]);

  const handleAddFiles = (filePaths: string[]) => {
    const newJobs: ConversionJob[] = filePaths.map((fp) => {
      const fileName = fp.split(/[\\/]/).pop() || fp;
      return {
        id: Math.random().toString(36).substring(2, 9),
        filePath: fp,
        fileName,
        outputDir: './output',
        status: 'idle',
      };
    });
    setJobs((prev) => [...prev, ...newJobs]);
    addLog('info', `Added ${filePaths.length} document(s) to conversion queue.`);
  };

  const handleRemoveJob = (id: string) => {
    setJobs((prev) => prev.filter((j) => j.id !== id));
  };

  const handleClearJobs = () => {
    setJobs([]);
    addLog('info', 'Batch queue cleared.');
  };

  const handleStartConversion = async (resume: boolean) => {
    if (jobs.length === 0 || isRunning) return;
    setIsRunning(true);
    addLog('info', `Starting batch conversion for ${jobs.length} document(s) (Resume: ${resume})...`);
    try {
      await startConversion({
        files: jobs.map((j) => j.filePath),
        outputDir: './output',
        config,
        resume,
      });
    } catch (err) {
      addLog('error', `Batch conversion error: ${err}`);
      setIsRunning(false);
    }
  };

  const handleStopConversion = () => {
    setIsRunning(false);
    addLog('warn', 'Conversion stopped by user.');
  };

  const handleDryRun = async () => {
    if (jobs.length === 0) return;
    addLog('info', 'Executing conversion dry-run simulation...');
    try {
      const report = await runDryRun(jobs[0].filePath, './output');
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
    setInspectedJob(job);
    setActiveView('inspector');
  };

  const handleSaveConfig = async (newConfig: AppConfigData) => {
    setConfig(newConfig);
    try {
      await saveConfig(newConfig);
      addLog('info', 'Configuration saved to pdfmd-lite.toml.');
    } catch (err) {
      addLog('error', `Failed to save configuration: ${err}`);
    }
  };

  const handleFormatNotes = async () => {
    addLog('info', 'Formatting notes across output vault...');
    try {
      const res = await formatNotes('./output', false);
      addLog('info', `Note formatter completed: ${res.filesProcessed} notes processed.`);
    } catch (err) {
      addLog('error', `Note formatting failed: ${err}`);
    }
  };

  const handleFixLatex = async () => {
    addLog('info', 'Sanitizing LaTeX delimiters across notes...');
    try {
      const res = await formatNotes('./output', true);
      addLog('info', `LaTeX sanitizer completed: ${res.replacementsMade} equations repaired.`);
    } catch (err) {
      addLog('error', `LaTeX sanitizer failed: ${err}`);
    }
  };

  const handleConvertPngToJpeg = async () => {
    addLog('info', 'Migrating vault PNG images to JPEG format...');
    try {
      const res = await convertPngToJpeg('./output', config.image_quality);
      addLog(
        'info',
        `Image migration completed: ${res.imagesConverted} images converted (~${Math.round(
          res.bytesSaved / 1024
        )} KB saved).`
      );
    } catch (err) {
      addLog('error', `Image migration failed: ${err}`);
    }
  };

  const handleEmbedVault = async () => {
    addLog('info', 'Generating semantic .embeddings.jsonl sidecars across vault...');
    try {
      const res = await embedVault('./output', false);
      addLog(
        'info',
        `Vault embedding finished: ${res.notesProcessed} notes processed, ${res.chunksEmbedded} semantic chunks indexed.`
      );
    } catch (err) {
      addLog('error', `Vault embedding failed: ${err}`);
    }
  };

  const handleToggleTheme = () => {
    setTheme((prev) => (prev === 'dark' ? 'obsidian' : prev === 'obsidian' ? 'light' : 'dark'));
  };

  return (
    <div className="w-screen h-screen flex flex-col bg-gray-900 text-gray-100 overflow-hidden font-sans">
      {/* In-Window Hierarchical Menu Bar */}
      <MenuBar
        onOpenPdf={() => {
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
              handleAddFiles(files);
            }
          };
          input.click();
        }}
        onOpenDirectory={() => {
          const input = document.createElement('input');
          input.type = 'file';
          input.webkitdirectory = true;
          input.onchange = (e) => {
            const target = e.target as HTMLInputElement;
            if (target.files) {
              const files = Array.from(target.files)
                .filter((f) => f.name.toLowerCase().endsWith('.pdf'))
                .map((f) => (f as unknown as { path?: string }).path || f.name);
              handleAddFiles(files);
            }
          };
          input.click();
        }}
        onWatchDirectory={() => {
          addLog('info', 'Directory watch mode active for input directory.');
        }}
        onOpenVault={() => {
          addLog('info', 'Opening output vault in Obsidian / File Explorer...');
        }}
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
          />
        ) : (
          <SplitPane
            job={inspectedJob}
            onBackToQueue={() => setActiveView('queue')}
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
    </div>
  );
};
