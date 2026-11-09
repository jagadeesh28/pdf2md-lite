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

    // 6. Obsidian and Markdown images
    html = html.replace(/!\[\[(.*?)\]\]/g, '<div class="my-3 p-3 bg-gray-800/60 rounded border border-gray-700 text-sm text-purple-300">🖼️ Image: $1</div>');
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
