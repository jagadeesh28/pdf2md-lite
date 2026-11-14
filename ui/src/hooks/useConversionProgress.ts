import { useState, useEffect, useCallback } from 'react';
import { BatchCompletionSummary, ConversionJob, JobStatus, LogMessage, ProgressPayload } from '../types';
import { subscribeProgress } from '../bridge/api';

export function useConversionProgress() {
  const [jobs, setJobs] = useState<ConversionJob[]>([]);
  const [logs, setLogs] = useState<LogMessage[]>([]);
  const [isRunning, setIsRunning] = useState<boolean>(false);
  const [currentEta, setCurrentEta] = useState<number | undefined>(undefined);
  const [pagesPerSec, setPagesPerSec] = useState<number | undefined>(undefined);
  const [activeDocSlug, setActiveDocSlug] = useState<string | undefined>(undefined);
  const [batchCompletedPages, setBatchCompletedPages] = useState<number | undefined>(undefined);
  const [batchTotalPages, setBatchTotalPages] = useState<number | undefined>(undefined);
  const [completionSummary, setCompletionSummary] = useState<BatchCompletionSummary | null>(null);

  const addLog = useCallback((level: LogMessage['level'], message: string) => {
    const newLog: LogMessage = {
      id: Math.random().toString(36).substring(2, 9),
      timestamp: new Date().toLocaleTimeString(),
      level,
      message,
    };
    setLogs((prev) => [...prev.slice(-999), newLog]);
  }, []);

  const clearLogs = useCallback(() => {
    setLogs([]);
  }, []);

  const dismissCompletionSummary = useCallback(() => {
    setCompletionSummary(null);
  }, []);

  const updateJobStatus = useCallback((id: string, status: JobStatus, error?: string) => {
    setJobs((prev) =>
      prev.map((j) => (j.id === id ? { ...j, status, error } : j))
    );
  }, []);

  // Real-time second-by-second countdown for active ETA display
  useEffect(() => {
    if (!isRunning) return;
    const timer = setInterval(() => {
      setCurrentEta((prev) => {
        if (prev === undefined || prev <= 0) return prev;
        return Math.max(0, prev - 1);
      });
    }, 1000);
    return () => clearInterval(timer);
  }, [isRunning]);

  useEffect(() => {
    const matchesJob = (j: ConversionJob, slug?: string, jobCount?: number) => {
      if (!slug) return false;
      if (jobCount === 1) return true; // Single document in queue unconditionally matches
      if (j.id === slug) return true;
      const cleanName = j.fileName.replace(/\.pdf$/i, '');
      if (j.fileName.includes(slug) || slug.includes(cleanName)) return true;
      if (j.filePath.includes(slug)) return true;
      const normSlug = slug.toLowerCase().replace(/[^a-z0-9]/g, '');
      const normName = cleanName.toLowerCase().replace(/[^a-z0-9]/g, '');
      if (normSlug && normName && (normSlug.includes(normName) || normName.includes(normSlug))) return true;
      const normPath = j.filePath.toLowerCase().replace(/[^a-z0-9]/g, '');
      if (normSlug && normPath && normPath.includes(normSlug)) return true;
      return false;
    };

    const unsubscribe = subscribeProgress((payload: ProgressPayload) => {
      if (payload.type === 'log') {
        if (payload.message) {
          addLog(payload.level || 'info', payload.message);
        }
      } else if (payload.type === 'doc_start') {
        setIsRunning(true);
        setCompletionSummary(null);
        if (payload.etaSec !== undefined && payload.etaSec > 0) {
          setCurrentEta(payload.etaSec);
        } else if (payload.totalPages && payload.totalPages > 0) {
          setCurrentEta(Math.max(2, payload.totalPages * 1.8));
        }
        if (payload.docSlug) {
          setActiveDocSlug(payload.docSlug);
          setJobs((prev) =>
            prev.map((j) =>
              matchesJob(j, payload.docSlug, prev.length)
                ? {
                    ...j,
                    status: 'running' as JobStatus,
                    totalPages: payload.totalPages ?? j.totalPages,
                  }
                : j
            )
          );
        }
      } else if (payload.type === 'page_complete') {
        if (payload.pagesPerSec !== undefined) setPagesPerSec(payload.pagesPerSec);
        if (payload.etaSec !== undefined) setCurrentEta(payload.etaSec);
        if (payload.batchCompletedPages !== undefined) setBatchCompletedPages(payload.batchCompletedPages);
        if (payload.batchTotalPages !== undefined) setBatchTotalPages(payload.batchTotalPages);
        if (payload.docSlug) {
          setActiveDocSlug(payload.docSlug);
          setJobs((prev) =>
            prev.map((j) =>
              matchesJob(j, payload.docSlug, prev.length)
                ? {
                    ...j,
                    completedPages: payload.page,
                    totalPages: payload.totalPages ?? j.totalPages,
                  }
                : j
            )
          );
        }
      } else if (payload.type === 'doc_complete') {
        const isOk = payload.status !== 'error';
        if (payload.docSlug) {
          setJobs((prev) =>
            prev.map((j) =>
              matchesJob(j, payload.docSlug, prev.length)
                ? {
                    ...j,
                    status: (isOk ? 'completed' : 'error') as JobStatus,
                    error: isOk ? undefined : payload.message,
                    completedPages: isOk && j.totalPages ? j.totalPages : j.completedPages,
                    elapsedSec: payload.elapsedSec,
                    outputMarkdownPath: payload.outputPath,
                  }
                : j
            )
          );
        }
      } else if (payload.type === 'batch_summary') {
        setIsRunning(false);
        setCurrentEta(undefined);
        setPagesPerSec(undefined);
        setActiveDocSlug(undefined);
        const summary: BatchCompletionSummary = {
          completed: payload.completed ?? 0,
          errors: payload.errors ?? 0,
          elapsedSec: payload.elapsedSec,
          totalPages: payload.totalPages,
          outputDir: payload.outputDir,
          finishedAt: new Date().toLocaleTimeString(),
        };
        setCompletionSummary(summary);
        addLog(
          (payload.errors ?? 0) === 0 ? 'info' : 'warn',
          `Conversion complete: ${payload.completed ?? 0} converted, ${payload.errors ?? 0} error(s)${
            payload.elapsedSec ? ` in ${payload.elapsedSec.toFixed(1)}s` : ''
          }.${payload.outputDir ? ` Output: ${payload.outputDir}` : ''}`
        );
      }
    });

    return () => {
      unsubscribe();
    };
  }, [addLog]);

  return {
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
    updateJobStatus,
  };
}
