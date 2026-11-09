import React, { useState } from 'react';
import { X, Save, RotateCcw } from 'lucide-react';
import { AppConfigData } from '../types';
import { defaultConfig } from '../bridge/api';

interface SettingsModalProps {
  config: AppConfigData;
  onSave: (newConfig: AppConfigData) => void;
  onClose: () => void;
}

export const SettingsModal: React.FC<SettingsModalProps> = ({ config, onSave, onClose }) => {
  const [formData, setFormData] = useState<AppConfigData>({ ...config });

  const handleReset = () => {
    setFormData({ ...defaultConfig });
  };

  const handleSave = (e: React.FormEvent) => {
    e.preventDefault();
    onSave(formData);
    onClose();
  };

  return (
    <div className="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4">
      <div className="bg-gray-900 border border-gray-700 rounded-xl shadow-2xl w-full max-w-xl overflow-hidden flex flex-col max-h-[90vh]">
        {/* Header */}
        <div className="bg-gray-850 px-5 py-3 border-b border-gray-800 flex items-center justify-between">
          <h2 className="text-sm font-bold text-white flex items-center space-x-2">
            <span>pdfmd-lite Engine Configuration</span>
          </h2>
          <button onClick={onClose} className="text-gray-400 hover:text-white transition-colors">
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Form Body */}
        <form onSubmit={handleSave} className="flex-1 overflow-auto p-5 space-y-4 text-xs">
          {/* Section: OCR & Language */}
          <div className="space-y-3">
            <h3 className="text-[11px] font-bold text-purple-400 uppercase tracking-wider">
              Recognition & DOM Extraction
            </h3>

            <div className="grid grid-cols-2 gap-4">
              <div>
                <label className="block text-gray-300 font-medium mb-1">Language / Script (-l)</label>
                <select
                  value={formData.language}
                  onChange={(e) => setFormData({ ...formData, language: e.target.value })}
                  className="w-full bg-gray-800 border border-gray-700 rounded px-2.5 py-1.5 text-gray-200 focus:outline-none focus:border-purple-500"
                >
                  <option value="en">English (SVTR Latin)</option>
                  <option value="hi">Hindi (Devanagari)</option>
                  <option value="te">Telugu</option>
                  <option value="kn">Kannada</option>
                  <option value="ta">Tamil</option>
                  <option value="ml">Malayalam</option>
                  <option value="mr">Marathi</option>
                  <option value="bn">Bengali</option>
                  <option value="gu">Gujarati</option>
                  <option value="pa">Punjabi</option>
                  <option value="or">Odia</option>
                </select>
              </div>

              <div>
                <label className="block text-gray-300 font-medium mb-1">
                  DOM Mode (--dom-extraction)
                </label>
                <select
                  value={formData.dom_extraction}
                  onChange={(e) =>
                    setFormData({
                      ...formData,
                      dom_extraction: e.target.value as 'auto' | 'always' | 'never',
                    })
                  }
                  className="w-full bg-gray-800 border border-gray-700 rounded px-2.5 py-1.5 text-gray-200 focus:outline-none focus:border-purple-500"
                >
                  <option value="auto">Auto (Hybrid DOM with OCR fallback)</option>
                  <option value="always">Always (Require digital DOM text)</option>
                  <option value="never">Never (Pure Vision Indra OCR)</option>
                </select>
              </div>
            </div>
          </div>

          <div className="border-t border-gray-800 my-2" />

          {/* Section: Figure & Images */}
          <div className="space-y-3">
            <h3 className="text-[11px] font-bold text-purple-400 uppercase tracking-wider">
              Figures & Asset Layout
            </h3>

            <div className="grid grid-cols-2 gap-4">
              <div>
                <label className="block text-gray-300 font-medium mb-1">Image Format</label>
                <select
                  value={formData.image_format}
                  onChange={(e) =>
                    setFormData({ ...formData, image_format: e.target.value as 'jpeg' | 'png' })
                  }
                  className="w-full bg-gray-800 border border-gray-700 rounded px-2.5 py-1.5 text-gray-200 focus:outline-none focus:border-purple-500"
                >
                  <option value="jpeg">JPEG (Compressed ~70% smaller)</option>
                  <option value="png">PNG (Lossless)</option>
                </select>
              </div>

              <div>
                <label className="block text-gray-300 font-medium mb-1">
                  JPEG Quality: {formData.image_quality}%
                </label>
                <input
                  type="range"
                  min="1"
                  max="100"
                  value={formData.image_quality}
                  onChange={(e) =>
                    setFormData({ ...formData, image_quality: parseInt(e.target.value, 10) })
                  }
                  className="w-full accent-purple-500"
                />
              </div>
            </div>

            <div className="space-y-2 pt-1">
              <label className="flex items-center space-x-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={formData.embed_images_base64}
                  onChange={(e) =>
                    setFormData({ ...formData, embed_images_base64: e.target.checked })
                  }
                  className="rounded border-gray-700 text-purple-600 focus:ring-purple-500 bg-gray-800"
                />
                <span className="text-gray-200">
                  Embed Images Inline as Base64 Data URIs (--embed-images-base64)
                </span>
              </label>

              <label className="flex items-center space-x-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={formData.extract_tables}
                  onChange={(e) =>
                    setFormData({ ...formData, extract_tables: e.target.checked })
                  }
                  className="rounded border-gray-700 text-purple-600 focus:ring-purple-500 bg-gray-800"
                />
                <span className="text-gray-200">Extract Structured Tables into Markdown Grids</span>
              </label>

              <label className="flex items-center space-x-2 cursor-pointer">
                <input
                  type="checkbox"
                  checked={formData.table_crops}
                  onChange={(e) => setFormData({ ...formData, table_crops: e.target.checked })}
                  className="rounded border-gray-700 text-purple-600 focus:ring-purple-500 bg-gray-800"
                />
                <span className="text-gray-200">Save Visual Table Crop Images</span>
              </label>
            </div>
          </div>

          <div className="border-t border-gray-800 my-2" />

          {/* Section: Concurrency & Embeddings */}
          <div className="space-y-3">
            <h3 className="text-[11px] font-bold text-purple-400 uppercase tracking-wider">
              Concurrency & Semantic Vector RAG
            </h3>

            <div className="grid grid-cols-2 gap-4">
              <div>
                <label className="block text-gray-300 font-medium mb-1">
                  Worker Concurrency (-j)
                </label>
                <input
                  type="number"
                  min="1"
                  max="64"
                  placeholder="Auto (Recommended)"
                  value={formData.concurrency ?? ''}
                  onChange={(e) =>
                    setFormData({
                      ...formData,
                      concurrency: e.target.value ? parseInt(e.target.value, 10) : null,
                    })
                  }
                  className="w-full bg-gray-800 border border-gray-700 rounded px-2.5 py-1.5 text-gray-200 focus:outline-none focus:border-purple-500 placeholder-gray-500"
                />
              </div>

              <div>
                <label className="block text-gray-300 font-medium mb-1">Log Level</label>
                <select
                  value={formData.log_level}
                  onChange={(e) =>
                    setFormData({
                      ...formData,
                      log_level: e.target.value as AppConfigData['log_level'],
                    })
                  }
                  className="w-full bg-gray-800 border border-gray-700 rounded px-2.5 py-1.5 text-gray-200 focus:outline-none focus:border-purple-500"
                >
                  <option value="trace">Trace (Hyper-verbose)</option>
                  <option value="debug">Debug</option>
                  <option value="info">Info (Standard)</option>
                  <option value="warn">Warn</option>
                  <option value="error">Error only</option>
                </select>
              </div>
            </div>

            <label className="flex items-center space-x-2 cursor-pointer pt-1">
              <input
                type="checkbox"
                checked={formData.embed}
                onChange={(e) => setFormData({ ...formData, embed: e.target.checked })}
                className="rounded border-gray-700 text-purple-600 focus:ring-purple-500 bg-gray-800"
              />
              <span className="text-gray-200">
                Generate .embeddings.jsonl Vector Sidecars (--embed)
              </span>
            </label>
          </div>

          {/* Footer Buttons */}
          <div className="border-t border-gray-800 pt-4 flex items-center justify-between">
            <button
              type="button"
              onClick={handleReset}
              className="flex items-center space-x-1.5 px-3 py-1.5 bg-gray-800 hover:bg-gray-700 text-gray-300 rounded transition-colors"
            >
              <RotateCcw className="w-3.5 h-3.5" />
              <span>Reset Defaults</span>
            </button>

            <div className="flex items-center space-x-2">
              <button
                type="button"
                onClick={onClose}
                className="px-3 py-1.5 text-gray-400 hover:text-white transition-colors"
              >
                Cancel
              </button>
              <button
                type="submit"
                className="flex items-center space-x-1.5 px-4 py-1.5 bg-purple-600 hover:bg-purple-500 text-white rounded font-medium shadow-md transition-colors"
              >
                <Save className="w-3.5 h-3.5" />
                <span>Save Settings</span>
              </button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
};
