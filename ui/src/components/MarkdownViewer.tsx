import React, { useMemo } from 'react';
import katex from 'katex';
import { marked } from 'marked';
import DOMPurify from 'dompurify';

interface MarkdownViewerProps {
  content: string;
}

export const MarkdownViewer: React.FC<MarkdownViewerProps> = ({ content }) => {
  const renderedHtml = useMemo(() => {
    if (!content) return '<div class="text-gray-500 italic">No document content loaded.</div>';

    // 1. Stash and render KaTeX display and inline math to avoid parser interference
    const mathPlaceholders: Map<string, string> = new Map();
    let placeholderCounter = 0;

    let processed = content;

    // Display math $$...$$
    processed = processed.replace(/\$\$([\s\S]*?)\$\$/g, (_match, math) => {
      const key = `@@MATH_DISPLAY_${placeholderCounter++}@@`;
      try {
        const rendered = katex.renderToString(math.trim(), { displayMode: true, throwOnError: false });
        mathPlaceholders.set(key, rendered);
      } catch (e) {
        mathPlaceholders.set(
          key,
          `<pre class="bg-red-900/30 text-red-300 p-2 rounded">Error rendering equation: ${escapeHtml(String(e))}</pre>`
        );
      }
      return `\n\n${key}\n\n`;
    });

    // Inline math $...$
    processed = processed.replace(/(?<!\$)\$([^\$\n]+?)\$(?!\$)/g, (_match, math) => {
      const key = `@@MATH_INLINE_${placeholderCounter++}@@`;
      try {
        const rendered = katex.renderToString(math.trim(), { displayMode: false, throwOnError: false });
        mathPlaceholders.set(key, rendered);
      } catch {
        mathPlaceholders.set(key, `$${escapeHtml(math)}$`);
      }
      return key;
    });

    // 2. Page Demarcation Anchors: <!-- Page N -->
    processed = processed.replace(
      /<!-- Page (\d+) -->/g,
      (_match, p1) => {
        const key = `@@PAGE_ANCHOR_${placeholderCounter++}@@`;
        const anchorHtml = `<div id="page-anchor-${p1}" class="my-6 pt-2 border-b border-dashed border-gray-700/80 flex items-center justify-between">` +
          '<span class="text-[10px] uppercase tracking-widest text-gray-500 font-mono">Page Demarcation</span>' +
          `<span class="text-[10px] font-mono font-bold text-purple-400 bg-gray-800 border border-gray-700 px-2 py-0.5 rounded shadow-sm">Page ${p1}</span>` +
          '</div>';
        mathPlaceholders.set(key, anchorHtml);
        return `\n\n${key}\n\n`;
      }
    );

    // 3. Obsidian Callouts (> [!type][+-]? Title)
    processed = processed.replace(
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

        const safeTitle = escapeHtml(displayTitle);
        const renderedBody = marked.parse(cleanBody, { async: false }) as string;

        const key = `@@CALLOUT_${placeholderCounter++}@@`;
        let calloutHtml: string;
        if (isCollapsible) {
          calloutHtml = `<details ${defaultOpen ? 'open' : ''} class="my-4 rounded-lg border ${theme.border} ${theme.bg} p-3 text-xs shadow-sm">
            <summary class="font-bold cursor-pointer select-none ${theme.text} flex items-center space-x-2">
              <span>${theme.icon}</span>
              <span>${safeTitle}</span>
            </summary>
            <div class="mt-2 text-gray-200 leading-relaxed border-t border-gray-800/80 pt-2">${renderedBody}</div>
          </details>`;
        } else {
          calloutHtml = `<div class="my-4 rounded-lg border-l-4 border ${theme.border} ${theme.bg} p-3 text-xs shadow-sm">
            <div class="font-bold ${theme.text} flex items-center space-x-2 mb-1.5">
              <span>${theme.icon}</span>
              <span>${safeTitle}</span>
            </div>
            <div class="text-gray-200 leading-relaxed">${renderedBody}</div>
          </div>`;
        }
        mathPlaceholders.set(key, calloutHtml);
        return `\n\n${key}\n\n`;
      }
    );

    // 4. Obsidian wikilinks & image links
    processed = processed.replace(/!\[\[(.*?)\]\]/g, (_m, target) => {
      const key = `@@WIKI_IMG_${placeholderCounter++}@@`;
      mathPlaceholders.set(
        key,
        `<div class="my-3 p-3 bg-gray-800/60 rounded border border-gray-700 text-sm text-purple-300">🖼️ Image: ${escapeHtml(target.trim())}</div>`
      );
      return key;
    });

    processed = processed.replace(/\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g, (_m, target, alias) => {
      const label = alias?.trim() || target.trim();
      const key = `@@WIKI_LINK_${placeholderCounter++}@@`;
      mathPlaceholders.set(
        key,
        `<span class="inline-flex items-center space-x-1 px-1.5 py-0.5 rounded text-xs font-medium bg-purple-950/50 text-purple-300 hover:text-purple-200 border border-purple-700/50 cursor-pointer transition-colors" title="Wikilink to: ${escapeHtml(target.trim())}"><span>🔗</span><span>${escapeHtml(label)}</span></span>`
      );
      return key;
    });

    // 5. Parse remaining standard markdown through marked
    let html = marked.parse(processed, {
      async: false,
      gfm: true,
      breaks: true,
    }) as string;

    // 6. Restore placeholders
    mathPlaceholders.forEach((val, key) => {
      html = html.split(key).join(val);
    });

    // 7. Sanitize strictly with DOMPurify
    const cleanHtml = DOMPurify.sanitize(html, {
      ALLOWED_TAGS: [
        'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
        'p', 'a', 'span', 'strong', 'em', 'del', 'code', 'pre',
        'blockquote', 'ul', 'ol', 'li', 'hr', 'br',
        'table', 'thead', 'tbody', 'tr', 'th', 'td',
        'div', 'details', 'summary', 'figure', 'figcaption', 'img',
        // KaTeX math rendering tags
        'math', 'semantics', 'mrow', 'mi', 'mo', 'mn', 'msup', 'msub', 'msubsup',
        'mfrac', 'msqrt', 'mroot', 'mtable', 'mtr', 'mtd', 'mtext', 'annotation',
        'svg', 'path', 'line', 'rect', 'circle', 'text', 'g'
      ],
      ALLOWED_ATTR: [
        'class', 'id', 'href', 'src', 'alt', 'title', 'open',
        'style', 'aria-hidden', 'aria-label', 'role',
        'width', 'height', 'viewBox', 'd', 'fill', 'stroke', 'xmlns',
        'data-page'
      ],
      ALLOWED_URI_REGEXP: /^(?:(?:(?:f|ht)tps?|mailto|tel|callto|sms|cid|xmpp|data|blob|asset):|[^a-z]|[a-z+.\-]+(?:[^a-z+.\-:]|$))/i,
      FORBID_TAGS: ['script', 'iframe', 'object', 'embed', 'form', 'input', 'textarea', 'button'],
      FORBID_ATTR: ['onerror', 'onload', 'onclick', 'onmouseover', 'onfocus', 'onblur'],
    });

    return cleanHtml;
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
