import React, { useMemo } from 'react';
import katex from 'katex';

interface MarkdownViewerProps {
  content: string;
}

export const MarkdownViewer: React.FC<MarkdownViewerProps> = ({ content }) => {
  const renderedHtml = useMemo(() => {
    if (!content) return '<div class="text-gray-500 italic">No document content loaded.</div>';

    let html = content;

    // 1. Render display math $$...$$
    html = html.replace(/\$\$([\s\S]*?)\$\$/g, (_match, math) => {
      try {
        return katex.renderToString(math.trim(), { displayMode: true, throwOnError: false });
      } catch (e) {
        return `<pre class="bg-red-900/30 text-red-300 p-2 rounded">Error rendering equation: ${e}</pre>`;
      }
    });

    // 2. Render inline math $...$ (avoiding double $)
    html = html.replace(/(?<!\$)\$([^\$\n]+?)\$(?!\$)/g, (_match, math) => {
      try {
        return katex.renderToString(math.trim(), { displayMode: false, throwOnError: false });
      } catch {
        return `$${math}$`;
      }
    });

    // 2.5 Page Demarcation Anchors: <!-- Page N -->
    html = html.replace(
      /<!-- Page (\d+) -->/g,
      '<div id="page-anchor-$1" class="my-6 pt-2 border-b border-dashed border-gray-700/80 flex items-center justify-between">' +
        '<span class="text-[10px] uppercase tracking-widest text-gray-500 font-mono">Page Demarcation</span>' +
        '<span class="text-[10px] font-mono font-bold text-purple-400 bg-gray-800 border border-gray-700 px-2 py-0.5 rounded shadow-sm">Page $1</span>' +
      '</div>'
    );

    // 2.6 Render Obsidian Callouts (> [!type][+-]? Title)
    html = html.replace(
      /^>[ \t]*\[!([a-zA-Z0-9_-]+)\]([+-]?)(?:[ \t]+([^\n]*))?\n((?:>[ \t]*.*\n?)*)/gim,
      (_match, rawType, collapse, title, body) => {
        const type = rawType.toLowerCase();
        const displayTitle = title?.trim() || type.toUpperCase();
        const cleanBody = body
          .split('\n')
          .map((line: string) => line.replace(/^>[ \t]?/, ''))
          .join('\n')
          .trim();

        const colorMap: Record<string, { border: string; bg: string; text: string; icon: string }> = {
          note: { border: 'border-blue-500/60', bg: 'bg-blue-950/30', text: 'text-blue-300', icon: 'ℹ️' },
          info: { border: 'border-cyan-500/60', bg: 'bg-cyan-950/30', text: 'text-cyan-300', icon: '💡' },
          tip: { border: 'border-emerald-500/60', bg: 'bg-emerald-950/30', text: 'text-emerald-300', icon: '🔥' },
          important: { border: 'border-indigo-500/60', bg: 'bg-indigo-950/30', text: 'text-indigo-300', icon: '⚡' },
          warning: { border: 'border-amber-500/60', bg: 'bg-amber-950/30', text: 'text-amber-300', icon: '⚠️' },
          caution: { border: 'border-amber-600/60', bg: 'bg-amber-950/40', text: 'text-amber-400', icon: '🛑' },
          danger: { border: 'border-red-500/60', bg: 'bg-red-950/40', text: 'text-red-300', icon: '🚨' },
          error: { border: 'border-red-600/60', bg: 'bg-red-950/50', text: 'text-red-400', icon: '❌' },
          table: { border: 'border-teal-500/60', bg: 'bg-teal-950/30', text: 'text-teal-300', icon: '📊' },
          figure: { border: 'border-purple-500/60', bg: 'bg-purple-950/30', text: 'text-purple-300', icon: '🖼️' },
        };

        const theme = colorMap[type] || {
          border: 'border-purple-500/60',
          bg: 'bg-gray-850/60',
          text: 'text-purple-300',
          icon: '📝',
        };

        const isCollapsible = collapse === '-' || collapse === '+';
        const defaultOpen = collapse !== '-';

        if (isCollapsible) {
          return `<details ${defaultOpen ? 'open' : ''} class="my-4 rounded-lg border ${theme.border} ${theme.bg} p-3 text-xs shadow-sm">
            <summary class="font-bold cursor-pointer select-none ${theme.text} flex items-center space-x-2">
              <span>${theme.icon}</span>
              <span>${displayTitle}</span>
            </summary>
            <div class="mt-2 text-gray-200 leading-relaxed border-t border-gray-800/80 pt-2">${cleanBody}</div>
          </details>`;
        }

        return `<div class="my-4 rounded-lg border-l-4 border ${theme.border} ${theme.bg} p-3 text-xs shadow-sm">
          <div class="font-bold ${theme.text} flex items-center space-x-2 mb-1.5">
            <span>${theme.icon}</span>
            <span>${displayTitle}</span>
          </div>
          <div class="text-gray-200 leading-relaxed">${cleanBody}</div>
        </div>`;
      }
    );

    // 3. Render code blocks ```...```
    html = html.replace(/```([a-zA-Z0-9_-]*)\n([\s\S]*?)```/g, (_match, lang, code) => {
      return `<pre class="bg-gray-800 text-gray-200 p-4 rounded-lg my-3 overflow-x-auto text-sm font-mono border border-gray-700"><code class="language-${lang}">${escapeHtml(
        code.trim()
      )}</code></pre>`;
    });

    // 4. Render tables
    html = html.replace(/((?:\|[^\n]+\|\r?\n)+)/g, (tableBlock) => {
      const rows = tableBlock.trim().split(/\r?\n/).filter(r => r.trim().length > 0);
      if (rows.length < 2) return tableBlock;

      let tableHtml = '<div class="my-4 overflow-x-auto"><table class="w-full text-left border-collapse border border-gray-700 text-sm">';
      let isHeader = true;

      for (let i = 0; i < rows.length; i++) {
        const row = rows[i].trim();
        if (row.match(/^\|(?:\s*:?-+:?\s*\|)+$/)) {
          // Separator row
          isHeader = false;
          continue;
        }

        const cells = row.split('|').slice(1, -1);
        if (isHeader) {
          tableHtml += '<thead class="bg-gray-800 text-gray-200"><tr>';
          cells.forEach((c) => {
            tableHtml += `<th class="border border-gray-700 px-3 py-2 font-semibold">${c.trim()}</th>`;
          });
          tableHtml += '</tr></thead><tbody>';
          isHeader = false;
        } else {
          tableHtml += '<tr class="hover:bg-gray-800/40 border-b border-gray-800">';
          cells.forEach((c) => {
            tableHtml += `<td class="border border-gray-700 px-3 py-2">${c.trim()}</td>`;
          });
          tableHtml += '</tr>';
        }
      }
      tableHtml += '</tbody></table></div>';
      return tableHtml;
    });

    // 5. Headings
    html = html.replace(/^### (.*$)/gim, '<h3 class="text-lg font-bold text-purple-400 mt-4 mb-2">$1</h3>');
    html = html.replace(/^## (.*$)/gim, '<h2 class="text-xl font-bold text-gray-100 border-b border-gray-800 pb-1 mt-6 mb-3">$1</h2>');
    html = html.replace(/^# (.*$)/gim, '<h1 class="text-2xl font-extrabold text-white mt-8 mb-4">$1</h1>');

    // 6. Obsidian and Markdown images & Wikilinks
    html = html.replace(/!\[\[(.*?)\]\]/g, '<div class="my-3 p-3 bg-gray-800/60 rounded border border-gray-700 text-sm text-purple-300">🖼️ Image: $1</div>');
    html = html.replace(/\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g, (_m, target, alias) => {
      const label = alias?.trim() || target.trim();
      return `<span class="inline-flex items-center space-x-1 px-1.5 py-0.5 rounded text-xs font-medium bg-purple-950/50 text-purple-300 hover:text-purple-200 border border-purple-700/50 cursor-pointer transition-colors" title="Wikilink to: ${target.trim()}"><span>🔗</span><span>${label}</span></span>`;
    });
    html = html.replace(/!\[(.*?)\]\((.*?)\)/g, (_match, alt, src) => {
      return `<figure class="my-4 text-center">
        <img src="${src}" alt="${alt}" class="max-w-full h-auto rounded border border-gray-700 mx-auto shadow-md" />
        <figcaption class="text-xs text-gray-400 mt-1 italic">${alt || 'Figure'}</figcaption>
      </figure>`;
    });

    // 7. Bold and Italics
    html = html.replace(/\*\*(.*?)\*\*/g, '<strong class="font-bold text-gray-100">$1</strong>');
    html = html.replace(/\*(.*?)\*/g, '<em class="italic text-gray-300">$1</em>');

    // 8. Paragraphs & Line Breaks
    const paragraphs = html.split(/\n\n+/);
    html = paragraphs
      .map((p) => {
        p = p.trim();
        if (p.startsWith('<h') || p.startsWith('<pre') || p.startsWith('<div') || p.startsWith('<table') || p.startsWith('<figure')) {
          return p;
        }
        return `<p class="my-2 leading-relaxed text-gray-300">${p.replace(/\n/g, '<br/>')}</p>`;
      })
      .join('\n');

    return html;
  }, [content]);

  return (
    <div
      className="prose prose-invert max-w-none px-6 py-4 selection:bg-purple-600 selection:text-white"
      dangerouslySetInnerHTML={{ __html: renderedHtml }}
    />
  );
};

function escapeHtml(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#039;');
}
