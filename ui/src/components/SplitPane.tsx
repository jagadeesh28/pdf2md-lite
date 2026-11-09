import React, { useState } from 'react';
import {
  ChevronLeft,
  ChevronRight,
  Copy,
  Check,
  Code,
  Eye,
  Download,
  FileText,
} from 'lucide-react';
import { ConversionJob } from '../types';
import { MarkdownViewer } from './MarkdownViewer';

interface SplitPaneProps {
  job: ConversionJob | null;
  onBackToQueue: () => void;
}

export const SplitPane: React.FC<SplitPaneProps> = ({ job, onBackToQueue }) => {
  const [currentPage, setCurrentPage] = useState<number>(1);
  const [viewMode, setViewMode] = useState<'rendered' | 'raw'>('rendered');
  const [copied, setCopied] = useState<boolean>(false);

  if (!job) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center bg-gray-900 text-gray-400 p-8">
        <FileText className="w-12 h-12 text-gray-600 mb-3" />
        <h3 className="text-sm font-semibold text-gray-200">No Document Selected</h3>
        <p className="text-xs text-gray-500 mt-1 mb-4">
          Select a completed document from the Batch Queue to inspect side-by-side.
        </p>
        <button
          onClick={onBackToQueue}
          className="px-3 py-1.5 bg-purple-600 hover:bg-purple-500 text-white rounded text-xs transition-colors"
        >
          Back to Batch Queue
        </button>
      </div>
    );
  }

  const pages = job.pages || [
    {
      pageNumber: 1,
      markdown: job.markdownContent || '# Sample Note\n\nNo page breakdown available.',
    },
  ];

  const totalPages = pages.length;
  const activePage = pages[currentPage - 1] || pages[0];

  const handleCopyMarkdown = () => {
    navigator.clipboard.writeText(activePage.markdown);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleExportMarkdown = () => {
    const blob = new Blob([job.markdownContent || activePage.markdown], {
      type: 'text/markdown;charset=utf-8',
    });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `${job.fileName.replace(/\.pdf$/i, '')}.md`;
    link.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="flex-1 flex flex-col h-full bg-gray-900 overflow-hidden">
      {/* Top Inspector Bar */}
      <div className="bg-gray-800/90 border-b border-gray-700/80 px-4 py-2 flex items-center justify-between text-xs">
        <div className="flex items-center space-x-3 overflow-hidden">
          <button
            onClick={onBackToQueue}
            className="flex items-center space-x-1 text-purple-400 hover:text-purple-300 font-medium transition-colors"
          >
            <ChevronLeft className="w-4 h-4" />
            <span>Queue</span>
          </button>
          <span className="text-gray-600">|</span>
          <span className="font-semibold text-gray-200 truncate">{job.fileName}</span>
        </div>

        {/* Page Switcher and View Toggle */}
        <div className="flex items-center space-x-3">
          <div className="flex items-center space-x-1 bg-gray-900/80 px-2 py-1 rounded border border-gray-700">
            <button
              disabled={currentPage <= 1}
              onClick={() => setCurrentPage((p) => Math.max(1, p - 1))}
              className="text-gray-400 hover:text-white disabled:opacity-30 transition-colors p-0.5"
            >
              <ChevronLeft className="w-4 h-4" />
            </button>
            <span className="text-xs font-mono text-gray-300 px-2">
              Page {currentPage} / {totalPages}
            </span>
            <button
              disabled={currentPage >= totalPages}
              onClick={() => setCurrentPage((p) => Math.min(totalPages, p + 1))}
              className="text-gray-400 hover:text-white disabled:opacity-30 transition-colors p-0.5"
            >
              <ChevronRight className="w-4 h-4" />
            </button>
          </div>

          <div className="flex items-center bg-gray-900 rounded border border-gray-700 p-0.5 text-xs">
            <button
              onClick={() => setViewMode('rendered')}
              className={`px-2 py-0.5 rounded flex items-center space-x-1 transition-colors ${
                viewMode === 'rendered' ? 'bg-purple-600 text-white' : 'text-gray-400 hover:text-white'
              }`}
            >
              <Eye className="w-3.5 h-3.5" />
              <span>Preview</span>
            </button>
            <button
              onClick={() => setViewMode('raw')}
              className={`px-2 py-0.5 rounded flex items-center space-x-1 transition-colors ${
                viewMode === 'raw' ? 'bg-purple-600 text-white' : 'text-gray-400 hover:text-white'
              }`}
            >
              <Code className="w-3.5 h-3.5" />
              <span>Raw</span>
            </button>
          </div>

          <button
            onClick={handleCopyMarkdown}
            className="flex items-center space-x-1 px-2.5 py-1 bg-gray-700 hover:bg-gray-600 text-white rounded transition-colors"
            title="Copy page markdown"
          >
            {copied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
            <span>{copied ? 'Copied' : 'Copy'}</span>
          </button>

          <button
            onClick={handleExportMarkdown}
            className="flex items-center space-x-1 px-2.5 py-1 bg-purple-600 hover:bg-purple-500 text-white rounded transition-colors"
            title="Export full note markdown"
          >
            <Download className="w-3.5 h-3.5" />
            <span>Export</span>
          </button>
        </div>
      </div>

      {/* Split View Content Area */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left Pane: Source PDF Page Canvas Preview */}
        <div className="w-1/2 border-r border-gray-800 bg-gray-950 flex flex-col overflow-hidden">
          <div className="bg-gray-900 px-3 py-1.5 border-b border-gray-800 text-[11px] font-semibold text-gray-400 flex items-center justify-between">
            <span>SOURCE PDF CANVAS</span>
            <span className="font-mono text-[10px] text-gray-500">Page {currentPage}</span>
          </div>

          <div className="flex-1 overflow-auto p-4 flex items-center justify-center">
            {activePage.previewUrl ? (
              <img
                src={activePage.previewUrl}
                alt={`Page ${currentPage}`}
                className="max-h-full max-w-full object-contain rounded shadow-lg border border-gray-800"
              />
            ) : (
              <div className="text-center text-gray-500 p-6">
                <FileText className="w-12 h-12 text-gray-700 mx-auto mb-2" />
                <p className="text-xs">Direct PDF canvas preview</p>
                <p className="text-[11px] text-gray-600 mt-1">
                  Rasterized at 300 DPI during pipeline processing
                </p>
              </div>
            )}
          </div>
        </div>

        {/* Right Pane: Generated Markdown & KaTeX */}
        <div className="w-1/2 bg-gray-900 flex flex-col overflow-hidden">
          <div className="bg-gray-800/60 px-3 py-1.5 border-b border-gray-800 text-[11px] font-semibold text-gray-400 flex items-center justify-between">
            <span>GENERATED OBSIDIAN MARKDOWN</span>
            <span className="font-mono text-[10px] text-purple-400">LaTeX & Tables</span>
          </div>

          <div className="flex-1 overflow-auto">
            {viewMode === 'rendered' ? (
              <MarkdownViewer content={activePage.markdown} />
            ) : (
              <pre className="p-4 text-xs font-mono text-gray-300 whitespace-pre-wrap select-text leading-relaxed">
                {activePage.markdown}
              </pre>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
