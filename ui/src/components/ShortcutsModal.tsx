import React, { useEffect } from 'react';
import { X, Keyboard } from 'lucide-react';

interface ShortcutsModalProps {
  onClose: () => void;
}

const shortcuts = [
  { key: 'Ctrl + O', description: 'Open single or multiple PDF documents' },
  { key: 'Ctrl + Shift + O', description: 'Open PDF directory for batch queue' },
  { key: 'Ctrl + Shift + V', description: 'Open output Markdown vault folder' },
  { key: 'Ctrl + S', description: 'Export currently inspected note' },
  { key: 'Ctrl + Enter', description: 'Start batch conversion' },
  { key: 'Ctrl + D', description: 'Run dry-run simulation without writes' },
  { key: 'Ctrl + 1', description: 'Switch to Split-Pane Inspector view' },
  { key: 'Ctrl + 2', description: 'Switch to Batch Queue Dashboard view' },
  { key: 'Ctrl + `', description: 'Toggle live telemetry log drawer' },
  { key: 'Ctrl + ,', description: 'Open engine settings configuration' },
  { key: 'F1', description: 'Open keyboard shortcuts cheatsheet' },
];

export const ShortcutsModal: React.FC<ShortcutsModalProps> = ({ onClose }) => {
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="shortcuts-modal-title"
    >
      <div className="bg-gray-900 border border-gray-700 rounded-xl shadow-2xl w-full max-w-lg overflow-hidden flex flex-col">
        {/* Header */}
        <div className="bg-gray-850 px-5 py-3 border-b border-gray-800 flex items-center justify-between">
          <h2 id="shortcuts-modal-title" className="text-sm font-bold text-white flex items-center space-x-2">
            <Keyboard className="w-4 h-4 text-purple-400" />
            <span>Keyboard Shortcuts</span>
          </h2>
          <button
            onClick={onClose}
            aria-label="Close shortcuts dialog"
            className="text-gray-400 hover:text-white transition-colors"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* List */}
        <div className="p-5 overflow-auto max-h-[70vh] space-y-2 text-xs">
          {shortcuts.map((s, idx) => (
            <div
              key={idx}
              className="flex items-center justify-between py-1.5 border-b border-gray-800/80"
            >
              <span className="text-gray-300">{s.description}</span>
              <kbd className="px-2 py-0.5 bg-gray-800 border border-gray-700 rounded text-purple-300 font-mono text-[11px] shadow-sm">
                {s.key}
              </kbd>
            </div>
          ))}
        </div>

        {/* Footer */}
        <div className="bg-gray-850 px-5 py-3 border-t border-gray-800 flex justify-end">
          <button
            onClick={onClose}
            className="px-4 py-1.5 bg-purple-600 hover:bg-purple-500 text-white rounded text-xs font-medium transition-colors"
          >
            Got it
          </button>
        </div>
      </div>
    </div>
  );
};
