import React, { useState, useEffect, useRef } from 'react';
import {
  ChevronLeft,
  ChevronRight,
  Copy,
  Check,
  Code,
  Eye,
  Download,
  FileText,
  Loader2,
} from 'lucide-react';
import { ConversionJob } from '../types';
import { MarkdownViewer } from './MarkdownViewer';
import { fetchPagePreview, readMarkdown } from '../bridge/api';

interface SplitPaneProps {
  job: ConversionJob | null;
  onBackToQueue: () => void;
}

export const SplitPane: React.FC<SplitPaneProps> = ({ job, onBackToQueue }) => {
  const [currentPage, setCurrentPage] = useState<number>(1);
  const [viewMode, setViewMode] = useState<'rendered' | 'raw'>('rendered');
  const [copied, setCopied] = useState<boolean>(false);
  const [previewSrc, setPreviewSrc] = useState<string | null>(null);
  const [loadingPreview, setLoadingPreview] = useState<boolean>(false);
  const [markdownContent, setMarkdownContent] = useState<string | null>(null);
  const [loadingMarkdown, setLoadingMarkdown] = useState<boolean>(false);
  const markdownPaneRef = useRef<HTMLDivElement>(null);
  const isScrollingFromCode = useRef<boolean>(false);

  useEffect(() => {
    if (!job) return;
    let active = true;
    setLoadingPreview(true);

    const targetFile = job.filePath || job.fileName;
    fetchPagePreview(targetFile, currentPage)
      .then((url) => {
        if (active) {
          setPreviewSrc(url);
          setLoadingPreview(false);
        }
      })
      .catch((err) => {
        console.error('Failed to load page preview:', err);
        if (active) {
          setLoadingPreview(false);
        }
      });

    return () => {
      active = false;
    };
  }, [job, currentPage]);

  useEffect(() => {
    if (markdownPaneRef.current) {
      const anchor = markdownPaneRef.current.querySelector(`#page-anchor-${currentPage}`);
      if (anchor) {
        anchor.scrollIntoView({ behavior: 'smooth', block: 'start' });
      }
    }
  }, [currentPage]);

  // Load the generated Markdown note for the selected completed document.
  useEffect(() => {
    if (!job) {
      setMarkdownContent(null);
      setLoadingMarkdown(false);
      return;
    }
    if (job.markdownContent) {
      setMarkdownContent(job.markdownContent);
      setLoadingMarkdown(false);
      return;
    }
    const sourcePath = job.outputMarkdownPath;
    if (!sourcePath) {
      setMarkdownContent(null);
      setLoadingMarkdown(false);
      return;
    }
    let active = true;
    setLoadingMarkdown(true);
    readMarkdown(sourcePath)
      .then((content) => {
        if (active) {
          setMarkdownContent(content);
          setLoadingMarkdown(false);
        }
      })
      .catch((err) => {
        console.error('Failed to load generated markdown:', err);
        if (active) {
          setLoadingMarkdown(false);
        }
      });
    return () => {
      active = false;
    };
  }, [job?.id, job?.outputMarkdownPath, job?.markdownContent]);

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

  const resolvedMarkdown = markdownContent ?? job.markdownContent ?? '';
  const pages =
    job.pages && job.pages.length > 0
      ? job.pages
      : resolvedMarkdown
      ? splitMarkdownIntoPages(resolvedMarkdown)
      : [
          {
            pageNumber: 1,
            markdown: '# Sample Note\n\nNo page breakdown available.',
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
    const content = resolvedMarkdown || activePage.markdown;
    const blob = new Blob([content], {
      type: 'text/markdown;charset=utf-8',
    });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = `${job.fileName.replace(/\.pdf$/i, '')}.md`;
    link.click();
    URL.revokeObjectURL(url);
  };

  const handlePageChange = (newPage: number) => {
    setCurrentPage(newPage);
    if (markdownPaneRef.current) {
      const anchor = markdownPaneRef.current.querySelector(`#page-anchor-${newPage}`);
      if (anchor) {
        isScrollingFromCode.current = true;
        anchor.scrollIntoView({ behavior: 'smooth', block: 'start' });
        setTimeout(() => {
          isScrollingFromCode.current = false;
        }, 400);
      }
    }
  };

  const handleMarkdownScroll = () => {
    if (isScrollingFromCode.current || !markdownPaneRef.current) return;
    const container = markdownPaneRef.current;
    const containerTop = container.scrollTop;

    for (const p of pages) {
      const el = container.querySelector<HTMLElement>(`#page-anchor-${p.pageNumber}`);
      if (el) {
        const offsetTop = el.offsetTop - container.offsetTop;
        if (containerTop >= offsetTop - 60 && containerTop < offsetTop + el.offsetHeight) {
          if (currentPage !== p.pageNumber) {
            setCurrentPage(p.pageNumber);
          }
          break;
        }
      }
    }
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
              onClick={() => handlePageChange(Math.max(1, currentPage - 1))}
              className="text-gray-400 hover:text-white disabled:opacity-30 transition-colors p-0.5"
            >
              <ChevronLeft className="w-4 h-4" />
            </button>
            <span className="text-xs font-mono text-gray-300 px-2">
              Page {currentPage} / {totalPages}
            </span>
            <button
              disabled={currentPage >= totalPages}
              onClick={() => handlePageChange(Math.min(totalPages, currentPage + 1))}
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

          <div className="flex-1 overflow-auto p-4 flex items-center justify-center bg-gray-950/80">
            {loadingPreview ? (
              <div className="text-center text-gray-500 p-6 flex flex-col items-center">
                <Loader2 className="w-8 h-8 text-purple-500 animate-spin mb-2" />
                <p className="text-xs">Rasterizing Page {currentPage} Preview...</p>
              </div>
            ) : previewSrc || activePage.previewUrl ? (
              <img
                src={previewSrc || activePage.previewUrl}
                alt={`Page ${currentPage}`}
                className="max-h-full max-w-full object-contain rounded shadow-2xl border border-gray-800"
              />
            ) : (
              <div className="text-center text-gray-500 p-6">
                <FileText className="w-12 h-12 text-gray-700 mx-auto mb-2" />
                <p className="text-xs">Direct PDF canvas preview</p>
                <p className="text-[11px] text-gray-600 mt-1">
                  Rasterized at 1.5× scale via hayro in-memory engine
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

          <div
            ref={markdownPaneRef}
            onScroll={handleMarkdownScroll}
            className="flex-1 overflow-auto"
          >
            {loadingMarkdown ? (
              <div className="h-full flex flex-col items-center justify-center text-gray-500 p-6">
                <Loader2 className="w-8 h-8 text-purple-500 animate-spin mb-2" />
                <p className="text-xs">Loading generated Markdown...</p>
              </div>
            ) : viewMode === 'rendered' ? (
              <div className="p-4 space-y-6">
                {pages.map((p) => (
                  <div
                    key={p.pageNumber}
                    id={`page-anchor-${p.pageNumber}`}
                    className="relative border-b border-gray-800 pb-6 last:border-b-0"
                  >
                    <div className="sticky top-0 bg-gray-900/90 backdrop-blur-sm py-1 mb-2 text-[10px] font-mono text-purple-400 font-semibold border-b border-gray-800/60 z-10 flex items-center justify-between">
                      <span>PAGE {p.pageNumber}</span>
                      <span className="text-gray-500 text-[9px]">Markdown Demarcation</span>
                    </div>
                    <MarkdownViewer content={p.markdown} />
                  </div>
                ))}
              </div>
            ) : (
              <pre className="p-4 text-xs font-mono text-gray-300 whitespace-pre-wrap select-text leading-relaxed">
                {resolvedMarkdown || activePage.markdown}
              </pre>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};

/**
 * Splits a generated note into per-page segments using the engine's
 * `<!-- Page N -->` demarcation markers so the inspector can page through
 * the PDF canvas and the Markdown side-by-side. Any content preceding the
 * first marker (e.g. YAML frontmatter and the document title) is folded into
 * page 1.
 */
function splitMarkdownIntoPages(
  content: string
): NonNullable<ConversionJob['pages']> {
  const markerRegex = /<!--\s*Page\s+(\d+)\s*-->/gi;
  const markers: { index: number; page: number }[] = [];
  let match: RegExpExecArray | null;
  while ((match = markerRegex.exec(content)) !== null) {
    markers.push({
      index: match.index,
      page: Number.parseInt(match[1], 10) || markers.length + 1,
    });
    if (match.index === markerRegex.lastIndex) {
      markerRegex.lastIndex++;
    }
  }

  if (markers.length === 0) {
    return [{ pageNumber: 1, markdown: content.trim() }];
  }

  const pages: NonNullable<ConversionJob['pages']> = [];
  let cursor = 0;
  for (let i = 0; i < markers.length; i++) {
    const nextStart = i + 1 < markers.length ? markers[i + 1].index : content.length;
    pages.push({
      pageNumber: markers[i].page,
      markdown: content.slice(cursor, nextStart).trim(),
    });
    cursor = nextStart;
  }
  return pages;
}
