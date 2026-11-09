import React, { useState, useRef, useEffect } from 'react';
import { Terminal, Trash2, ArrowDown, X } from 'lucide-react';
import { LogMessage } from '../types';

interface LogConsoleProps {
  logs: LogMessage[];
  onClear: () => void;
  onClose: () => void;
}

export const LogConsole: React.FC<LogConsoleProps> = ({ logs, onClear, onClose }) => {
  const [filter, setFilter] = useState<string>('');
  const [autoScroll, setAutoScroll] = useState<boolean>(true);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (autoScroll && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [logs, autoScroll]);

  const filteredLogs = filter
    ? logs.filter(
        (l) =>
          l.message.toLowerCase().includes(filter.toLowerCase()) ||
          l.level.toLowerCase().includes(filter.toLowerCase())
      )
    : logs;

  const getLevelColor = (level: LogMessage['level']) => {
    switch (level) {
      case 'error':
        return 'text-red-400 font-bold';
      case 'warn':
        return 'text-amber-400 font-medium';
      case 'debug':
        return 'text-cyan-400';
      case 'trace':
        return 'text-gray-500';
      default:
        return 'text-gray-300';
    }
  };

  return (
    <div className="h-56 bg-gray-950 border-t border-gray-800 flex flex-col font-mono text-xs select-text">
      {/* Console Header */}
      <div className="bg-gray-900 px-3 py-1.5 border-b border-gray-800 flex items-center justify-between text-gray-400">
        <div className="flex items-center space-x-2">
          <Terminal className="w-3.5 h-3.5 text-purple-400" />
          <span className="font-semibold text-gray-200 text-[11px]">TELEMETRY & LOG STREAM</span>
          <span className="text-[10px] bg-gray-800 px-1.5 py-0.2 rounded text-gray-400">
            {filteredLogs.length} events
          </span>
        </div>

        <div className="flex items-center space-x-2">
          <input
            type="text"
            placeholder="Filter logs..."
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            className="bg-gray-800 text-gray-200 px-2 py-0.5 rounded text-[11px] border border-gray-700 focus:outline-none focus:border-purple-500 w-36"
          />

          <button
            onClick={() => setAutoScroll((v) => !v)}
            className={`p-1 rounded transition-colors ${
              autoScroll ? 'text-purple-400 bg-purple-950/40' : 'text-gray-500 hover:text-gray-300'
            }`}
            title={autoScroll ? 'Auto-scroll enabled' : 'Auto-scroll paused'}
          >
            <ArrowDown className="w-3.5 h-3.5" />
          </button>

          <button
            onClick={onClear}
            className="text-gray-500 hover:text-gray-300 p-1 transition-colors"
            title="Clear logs"
          >
            <Trash2 className="w-3.5 h-3.5" />
          </button>

          <button
            onClick={onClose}
            className="text-gray-500 hover:text-gray-300 p-1 transition-colors ml-1"
            title="Close console"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Log Output Area */}
      <div ref={containerRef} className="flex-1 overflow-auto p-2.5 space-y-1">
        {filteredLogs.length === 0 ? (
          <div className="text-gray-600 italic">No telemetry messages logged.</div>
        ) : (
          filteredLogs.map((log) => (
            <div key={log.id} className="leading-tight flex items-start space-x-2">
              <span className="text-gray-600 text-[10px] flex-shrink-0 select-none">
                [{log.timestamp}]
              </span>
              <span
                className={`text-[10px] uppercase font-bold flex-shrink-0 w-12 ${getLevelColor(
                  log.level
                )}`}
              >
                {log.level}
              </span>
              <span className={`break-all ${getLevelColor(log.level)}`}>{log.message}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
};
