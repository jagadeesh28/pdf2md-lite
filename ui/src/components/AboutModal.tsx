import React, { useEffect } from 'react';
import { X, ShieldCheck, Cpu, Code2 } from 'lucide-react';

interface AboutModalProps {
  onClose: () => void;
}

export const AboutModal: React.FC<AboutModalProps> = ({ onClose }) => {
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
      aria-labelledby="about-modal-title"
    >
      <div className="bg-gray-900 border border-gray-700 rounded-xl shadow-2xl w-full max-w-md overflow-hidden flex flex-col">
        {/* Header */}
        <div className="bg-gray-850 px-5 py-3 border-b border-gray-800 flex items-center justify-between">
          <div className="flex items-center space-x-2">
            <span className="w-2.5 h-2.5 rounded-full bg-purple-500 animate-pulse" />
            <h2 id="about-modal-title" className="text-sm font-bold text-white">About pdfmd-lite</h2>
          </div>
          <button
            onClick={onClose}
            aria-label="Close about dialog"
            className="text-gray-400 hover:text-white transition-colors"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Content */}
        <div className="p-6 space-y-4 text-xs text-gray-300 leading-relaxed">
          <div className="text-center pb-2">
            <h1 className="text-lg font-extrabold text-white">pdfmd-lite</h1>
            <p className="text-purple-400 font-mono text-[11px] mt-0.5">Version 0.7.2 • Pure-Rust Architecture</p>
          </div>

          <p>
            An autonomous, high-performance offline document conversion engine transforming scientific
            and technical PDFs into Obsidian-flavored Markdown with native LaTeX mathematics and structured tables.
          </p>

          <div className="bg-gray-800/60 rounded-lg p-3 border border-gray-700/60 space-y-2 text-[11px]">
            <div className="flex items-center space-x-2 text-emerald-400">
              <ShieldCheck className="w-4 h-4 flex-shrink-0" />
              <span>100% Offline • Zero External LLM or Cloud Telemetry</span>
            </div>
            <div className="flex items-center space-x-2 text-indigo-400">
              <Cpu className="w-4 h-4 flex-shrink-0" />
              <span>tract-onnx Pure SIMD + Optional ort Hardware Acceleration</span>
            </div>
            <div className="flex items-center space-x-2 text-amber-400">
              <Code2 className="w-4 h-4 flex-shrink-0" />
              <span>Zero System Environment Variables Rule</span>
            </div>
          </div>

          <div className="text-center text-[11px] text-gray-500 pt-2 border-t border-gray-800">
            Licensed under Apache-2.0 / MIT.
          </div>
        </div>

        {/* Footer */}
        <div className="bg-gray-850 px-5 py-3 border-t border-gray-800 flex justify-end">
          <button
            onClick={onClose}
            className="px-4 py-1.5 bg-gray-800 hover:bg-gray-700 text-gray-200 rounded text-xs transition-colors"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
};
