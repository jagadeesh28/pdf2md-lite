import { describe, it, expect, vi } from 'vitest';
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { SettingsModal } from './SettingsModal';
import { AboutModal } from './AboutModal';
import { ShortcutsModal } from './ShortcutsModal';
import { ModelsModal } from './ModelsModal';
import { defaultConfig } from '../bridge/api';

vi.mock('../bridge/api', async () => {
  const actual = await vi.importActual<Record<string, unknown>>('../bridge/api');
  return {
    ...actual,
    getModelStatus: vi.fn().mockResolvedValue([]),
    downloadModels: vi.fn().mockResolvedValue({ message: 'Success' }),
  };
});

function renderModal(ui: React.ReactElement): HTMLElement {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => {
    root.render(ui);
  });
  return container;
}

describe('Modal Accessibility & Keyboard Handling', () => {
  it('SettingsModal has role="dialog", aria-modal="true", and closes on Escape', () => {
    const handleClose = vi.fn();
    const handleSave = vi.fn();
    const container = renderModal(
      <SettingsModal
        onClose={handleClose}
        config={defaultConfig}
        onSave={handleSave}
      />
    );

    const dialog = container.querySelector('[role="dialog"]');
    expect(dialog).not.toBeNull();
    expect(dialog?.getAttribute('aria-modal')).toBe('true');
    expect(dialog?.getAttribute('aria-labelledby')).toBe('settings-modal-title');

    // Trigger Escape key
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(handleClose).toHaveBeenCalledTimes(1);
  });

  it('AboutModal displays version 0.7.2, role="dialog", and closes on Escape', () => {
    const handleClose = vi.fn();
    const container = renderModal(
      <AboutModal onClose={handleClose} />
    );

    const dialog = container.querySelector('[role="dialog"]');
    expect(dialog).not.toBeNull();
    expect(container.innerHTML).toContain('0.7.2');

    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(handleClose).toHaveBeenCalledTimes(1);
  });

  it('ShortcutsModal has dialog accessibility and closes on Escape', () => {
    const handleClose = vi.fn();
    const container = renderModal(
      <ShortcutsModal onClose={handleClose} />
    );

    const dialog = container.querySelector('[role="dialog"]');
    expect(dialog).not.toBeNull();

    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(handleClose).toHaveBeenCalledTimes(1);
  });

  it('ModelsModal has dialog accessibility and closes on Escape', () => {
    const handleClose = vi.fn();
    const container = renderModal(
      <ModelsModal onClose={handleClose} />
    );

    const dialog = container.querySelector('[role="dialog"]');
    expect(dialog).not.toBeNull();

    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(handleClose).toHaveBeenCalledTimes(1);
  });
});
