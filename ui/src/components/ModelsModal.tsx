import React, { useState, useEffect } from 'react';
import { X, Download, CheckCircle2, AlertTriangle, AlertCircle, Cpu, HardDrive, RefreshCw } from 'lucide-react';
import { downloadModels, getModelStatus } from '../bridge/api';
import { ModelStatus } from '../types';

interface ModelsModalProps {
  onClose: () => void;
}

export const ModelsModal: React.FC<ModelsModalProps> = ({ onClose }) => {
  const [models, setModels] = useState<ModelStatus[]>([]);
  const [loading, setLoading] = useState<boolean>(true);
  const [downloading, setDownloading] = useState<boolean>(false);
  const [resultMessage, setResultMessage] = useState<string | null>(null);

  const loadModels = async () => {
    setLoading(true);
    try {
      const data = await getModelStatus();
      setModels(data);
    } catch (err) {
      console.error('Failed to load models:', err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadModels();
  }, []);

  const handleDownload = async () => {
    setDownloading(true);
    setResultMessage(null);
    try {
      const res = await downloadModels();
      setResultMessage(res.message || 'Models verified successfully.');
      await loadModels();
    } catch (err) {
      setResultMessage(`Operation failed: ${err}`);
    } finally {
      setDownloading(false);
    }
  };

  const formatSize = (bytes: number | null | undefined): string => {
    if (!bytes) return '-';
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  };

  return (
    <div className="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="bg-gray-900 border border-gray-700 rounded-xl shadow-2xl w-full max-w-2xl overflow-hidden flex flex-col max-h-[90vh]">
        {/* Header */}
        <div className="bg-gray-850 px-5 py-3 border-b border-gray-800 flex items-center justify-between">
          <h2 className="text-sm font-bold text-white flex items-center space-x-2">
            <Cpu className="w-4 h-4 text-purple-400" />
            <span>Offline Neural Model Registry & Integrity Diagnostics</span>
          </h2>
          <button onClick={onClose} className="text-gray-400 hover:text-white transition-colors">
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Content */}
        <div className="p-5 flex-1 overflow-auto space-y-4 text-xs">
          <p className="text-gray-300">
            All models execute 100% offline in-process via pure-Rust SIMD (`tract-onnx`) or hardware-accelerated `ort`.
            Checksums are verified against compile-time embedded <code>MANIFEST.toml</code> signatures.
          </p>

          {loading ? (
            <div className="flex items-center justify-center py-12 text-gray-400 space-x-2">
              <RefreshCw className="w-4 h-4 animate-spin text-purple-400" />
              <span>Inspecting local offline models...</span>
            </div>
          ) : (
            <div className="space-y-2.5">
              {models.map((m, idx) => {
                const isVerified = m.exists && m.is_valid;
                const isMismatch = m.exists && !m.is_valid;
                const isMissingRequired = !m.exists && m.required;
                const isMissingOptional = !m.exists && !m.required;

                return (
                  <div
                    key={idx}
                    className={`border rounded-lg p-3 flex items-start justify-between ${
                      isMismatch || isMissingRequired
                        ? 'bg-red-950/20 border-red-800/50'
                        : isVerified
                        ? 'bg-gray-800/60 border-gray-700/60'
                        : 'bg-gray-800/30 border-gray-750/40'
                    }`}
                  >
                    <div className="flex-1 pr-4">
                      <div className="flex items-center space-x-2">
                        <span className="font-semibold text-gray-200">{m.name}</span>
                        <span
                          className={`text-[10px] px-1.5 py-0.5 rounded font-mono border ${
                            m.required
                              ? 'bg-purple-950/80 text-purple-300 border-purple-800/60'
                              : 'bg-gray-800 text-gray-400 border-gray-700'
                          }`}
                        >
                          {m.required ? 'REQUIRED' : 'OPTIONAL'}
                        </span>
                        {m.is_embedded && (
                          <span className="text-[10px] bg-blue-950/80 text-blue-300 border border-blue-800/60 px-1.5 py-0.5 rounded font-mono">
                            EMBEDDED
                          </span>
                        )}
                      </div>
                      <p className="text-[11px] text-gray-400 mt-1 font-mono truncate">{m.subpath}</p>
                      {m.resolved_path && (
                        <p className="text-[10px] text-gray-500 font-mono truncate mt-0.5">
                          {m.resolved_path}
                        </p>
                      )}
                      {m.error && (
                        <p className="text-[11px] text-red-400 mt-1 flex items-center space-x-1">
                          <AlertCircle className="w-3 h-3 flex-shrink-0" />
                          <span>{m.error}</span>
                        </p>
                      )}
                    </div>

                    <div className="flex flex-col items-end space-y-1 flex-shrink-0 ml-3">
                      {isVerified && (
                        <span className="flex items-center space-x-1 text-emerald-400 text-[11px] font-medium">
                          <CheckCircle2 className="w-3.5 h-3.5" />
                          <span>Verified</span>
                        </span>
                      )}
                      {isMismatch && (
                        <span className="flex items-center space-x-1 text-amber-400 text-[11px] font-medium">
                          <AlertTriangle className="w-3.5 h-3.5" />
                          <span>Hash Mismatch</span>
                        </span>
                      )}
                      {isMissingRequired && (
                        <span className="flex items-center space-x-1 text-red-400 text-[11px] font-medium">
                          <AlertCircle className="w-3.5 h-3.5" />
                          <span>Missing Required</span>
                        </span>
                      )}
                      {isMissingOptional && (
                        <span className="text-gray-500 text-[11px] font-medium">
                          Not Installed
                        </span>
                      )}

                      <div className="flex items-center space-x-2 text-[10px] text-gray-500 font-mono">
                        <span className="flex items-center space-x-1">
                          <HardDrive className="w-3 h-3" />
                          <span>{formatSize(m.size_bytes)}</span>
                        </span>
                        {m.sha256 && <span>[{m.sha256.slice(0, 8)}]</span>}
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          {resultMessage && (
            <div className="p-3 bg-purple-950/40 border border-purple-700/50 rounded text-purple-200 text-xs">
              {resultMessage}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="bg-gray-850 px-5 py-3 border-t border-gray-800 flex items-center justify-between">
          <span className="text-[11px] text-gray-500">
            Offline AppData: <code>pdfmd-lite/models/</code>
          </span>

          <div className="flex items-center space-x-2">
            <button
              onClick={handleDownload}
              disabled={downloading}
              className="flex items-center space-x-1.5 px-3.5 py-1.5 bg-purple-600 hover:bg-purple-500 text-white rounded font-medium text-xs shadow transition-colors disabled:opacity-50"
            >
              <Download className="w-3.5 h-3.5" />
              <span>{downloading ? 'Processing...' : 'Verify / Provision Models'}</span>
            </button>
            <button
              onClick={onClose}
              className="px-3 py-1.5 bg-gray-800 hover:bg-gray-700 text-gray-300 rounded text-xs transition-colors"
            >
              Close
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
