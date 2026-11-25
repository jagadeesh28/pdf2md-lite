import { describe, it, expect } from 'vitest';
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { MarkdownViewer } from './MarkdownViewer';

function renderComponent(ui: React.ReactElement): HTMLElement {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => {
    root.render(ui);
  });
  return container;
}

describe('MarkdownViewer Security & Sanitization', () => {
  it('sanitizes script tags from markdown content', () => {
    const malicious = '# Hello\n<script>alert("pwned")</script>\nNormal text';
    const container = renderComponent(<MarkdownViewer content={malicious} />);
    expect(container.querySelector('script')).toBeNull();
    expect(container.innerHTML).not.toContain('alert("pwned")');
  });

  it('strips onerror and dangerous event handlers on img and other tags', () => {
    const malicious = '<img src="invalid" onerror="window.__TAURI__.core.invoke(\'read_markdown\')" />\n<svg onload="alert(1)">';
    const container = renderComponent(<MarkdownViewer content={malicious} />);
    expect(container.innerHTML).not.toContain('onerror');
    expect(container.innerHTML).not.toContain('onload');
    expect(container.innerHTML).not.toContain('window.__TAURI__');
  });

  it('disallows javascript: URLs in links', () => {
    const malicious = '[Click me](javascript:alert("exploit"))';
    const container = renderComponent(<MarkdownViewer content={malicious} />);
    const link = container.querySelector('a');
    if (link) {
      const href = link.getAttribute('href');
      expect(href === null || !href.includes('javascript:')).toBe(true);
    }
  });

  it('safely escapes hostile attribute injections in wikilinks and images', () => {
    const malicious = '[[test" onmouseover="alert(1)|malicious]]\n![alt" onerror="alert(2)](x.jpg)';
    const container = renderComponent(<MarkdownViewer content={malicious} />);
    expect(container.querySelector('[onmouseover]')).toBeNull();
    expect(container.querySelector('[onerror]')).toBeNull();
  });

  it('renders KaTeX display and inline equations correctly', () => {
    const content = 'The formula is $E = mc^2$ and display:\n\n$$\\int_0^1 x^2 dx = \\frac{1}{3}$$';
    const container = renderComponent(<MarkdownViewer content={content} />);
    expect(container.querySelector('.katex')).not.toBeNull();
  });

  it('renders Obsidian callouts and page anchors', () => {
    const content = '> [!warning] Critical Notice\n> Please verify the payload.\n\n<!-- Page 42 -->';
    const container = renderComponent(<MarkdownViewer content={content} />);
    expect(container.innerHTML).toContain('Critical Notice');
    expect(container.querySelector('#page-anchor-42')).not.toBeNull();
  });
});
