import { useState, useEffect, useCallback } from 'react';
import { ConversionJob, JobStatus, LogMessage, ProgressPayload } from '../types';
import { subscribeProgress } from '../bridge/api';

export function useConversionProgress() {
  const [jobs, setJobs] = useState<ConversionJob[]>([]);
  const [logs, setLogs] = useState<LogMessage[]>([]);
  const [isRunning, setIsRunning] = useState<boolean>(false);
  const [currentEta, setCurrentEta] = useState<number | undefined>(undefined);
  const [pagesPerSec, setPagesPerSec] = useState<number | undefined>(undefined);

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

  const updateJobStatus = useCallback((id: string, status: JobStatus, error?: string) => {
    setJobs((prev) =>
      prev.map((j) => (j.id === id ? { ...j, status, error } : j))
    );
  }, []);

  useEffect(() => {
    const unsubscribe = subscribeProgress((payload: ProgressPayload) => {
      if (payload.type === 'log') {
        if (payload.message) {
          addLog(payload.level || 'info', payload.message);
        }
      } else if (payload.type === 'doc_start') {
        setIsRunning(true);
        if (payload.docSlug) {
          setJobs((prev) =>
            prev.map((j) =>
              j.fileName.includes(payload.docSlug!) || j.id === payload.docSlug
                ? { ...j, status: 'running' as JobStatus }
                : j
            )
          );
        }
      } else if (payload.type === 'page_complete') {
        if (payload.pagesPerSec !== undefined) setPagesPerSec(payload.pagesPerSec);
        if (payload.etaSec !== undefined) setCurrentEta(payload.etaSec);
        if (payload.docSlug) {
          setJobs((prev) =>
            prev.map((j) =>
              j.fileName.includes(payload.docSlug!) || j.id === payload.docSlug
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
              j.fileName.includes(payload.docSlug!) || j.id === payload.docSlug
                ? {
                    ...j,
                    status: (isOk ? 'completed' : 'error') as JobStatus,
                    error: isOk ? undefined : payload.message,
                  }
                : j
            )
          );
        }
      } else if (payload.type === 'batch_summary') {
        setIsRunning(false);
        setCurrentEta(undefined);
        setPagesPerSec(undefined);
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
    updateJobStatus,
  };
}
