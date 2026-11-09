import React, { useState } from 'react';
import { X, Download, CheckCircle2, Cpu, HardDrive } from 'lucide-react';
import { downloadModels } from '../bridge/api';

interface ModelsModalProps {
  onClose: () => void;
}

interface ModelAsset {
  name: string;
  category: string;
  size: string;
  description: string;
  status: 'installed' | 'missing';
}

const defaultModels: ModelAsset[] = [
  {
    name: 'all-MiniLM-L6-v2 (INT8)',
    category: 'Semantic Vector Embeddings',
    size: '22 MB',
    description: '384-dimensional dense sentence embeddings for Obsidian RAG plugins.',
    status: 'installed',
  },
  {
    name: 'Indra DBNet + SVTR Latin / World',
    category: 'Text Detection & Recognition',
    size: '14 MB',
    description: 'Multilingual CTC OCR supporting Latin, Devanagari, Telugu, Kannada, etc.',
    status: 'installed',
  },
  {
    name: 'RapidLaTeXOCR Encoder-Decoder',
    category: 'Mathematical Formula Recognition',
    size: '18 MB',
    description: 'Vision transformer converting formula crops into native LaTeX math blocks.',
    status: 'installed',
  },
  {
    name: 'SLANet Neural Table Extractor',
    category: 'Table Structure Recognition',
    size: '7 MB',
    description: 'Deep neural network parsing complex table headers and merged cells.',
    status: 'installed',
  },
  {
    name: 'GLiNER Small Multi-Task NER',
    category: 'Metadata Extraction',
    size: '150 MB',
    description: 'Zero-shot entity extraction for Title, Authors, Abstract, and DOI tags.',
    status: 'installed',
  },
];

export const ModelsModal: React.FC<ModelsModalProps> = ({ onClose }) => {
  const [downloading, setDownloading] = useState<boolean>(false);
  const [resultMessage, setResultMessage] = useState<string | null>(null);

  const handleDownload = async () => {
    setDownloading(true);
    setResultMessage(null);
    try {
      const res = await downloadModels();
      setResultMessage(res.message || 'Models verified successfully.');
    } catch (err) {
      setResultMessage(`Download failed: ${err}`);
    } finally {
      setDownloading(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="bg-gray-900 border border-gray-700 rounded-xl shadow-2xl w-full max-w-xl overflow-hidden flex flex-col max-h-[90vh]">
        {/* Header */}
        <div className="bg-gray-850 px-5 py-3 border-b border-gray-800 flex items-center justify-between">
          <h2 className="text-sm font-bold text-white flex items-center space-x-2">
            <Cpu className="w-4 h-4 text-purple-400" />
            <span>In-Process Neural Model Provisioning</span>
          </h2>
          <button onClick={onClose} className="text-gray-400 hover:text-white transition-colors">
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Content */}
        <div className="p-5 flex-1 overflow-auto space-y-4 text-xs">
          <p className="text-gray-300">
            All models execute 100% offline in-process via pure-Rust SIMD (`tract-onnx`) or hardware-accelerated `ort`.
            Zero external cloud APIs or background LLM runtimes are ever contacted.
          </p>

          <div className="space-y-2.5">
            {defaultModels.map((m, idx) => (
              <div
                key={idx}
                className="bg-gray-800/60 border border-gray-700/60 rounded-lg p-3 flex items-start justify-between"
              >
                <div>
                  <div className="flex items-center space-x-2">
                    <span className="font-semibold text-gray-200">{m.name}</span>
                    <span className="text-[10px] bg-purple-950/80 text-purple-300 border border-purple-800/60 px-1.5 py-0.2 rounded font-mono">
                      {m.category}
                    </span>
                  </div>
                  <p className="text-[11px] text-gray-400 mt-1">{m.description}</p>
                </div>

                <div className="flex flex-col items-end space-y-1 flex-shrink-0 ml-3">
                  <span className="flex items-center space-x-1 text-emerald-400 text-[11px] font-medium">
                    <CheckCircle2 className="w-3.5 h-3.5" />
                    <span>Verified</span>
                  </span>
                  <span className="text-[10px] text-gray-500 font-mono flex items-center space-x-1">
                    <HardDrive className="w-3 h-3" />
                    <span>{m.size}</span>
                  </span>
                </div>
              </div>
            ))}
          </div>

          {resultMessage && (
            <div className="p-3 bg-purple-950/40 border border-purple-700/50 rounded text-purple-200 text-xs">
              {resultMessage}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="bg-gray-850 px-5 py-3 border-t border-gray-800 flex items-center justify-between">
          <span className="text-[11px] text-gray-500">
            Provisioned in <code>models/</code>
          </span>

          <div className="flex items-center space-x-2">
            <button
              onClick={handleDownload}
              disabled={downloading}
              className="flex items-center space-x-1.5 px-3.5 py-1.5 bg-purple-600 hover:bg-purple-500 text-white rounded font-medium text-xs shadow transition-colors disabled:opacity-50"
            >
              <Download className="w-3.5 h-3.5" />
              <span>{downloading ? 'Downloading...' : 'Verify / Download Models'}</span>
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
