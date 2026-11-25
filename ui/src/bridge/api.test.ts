import { describe, it, expect } from 'vitest';
import { defaultConfig, normalizeConfig, getAssetUrl } from './api';

describe('Bridge API & Config Normalization', () => {
  it('defaultConfig has expected Phase 13 fields and defaults', () => {
    expect(defaultConfig.save_table_images).toBe(true);
    expect(defaultConfig.device).toBe('auto');
    expect(defaultConfig.dom_extraction).toBe('auto');
    expect(defaultConfig.concurrency).toBeNull();
    expect(defaultConfig.language).toBe('en');
  });

  it('normalizeConfig preserves and normalizes newly added fields', () => {
    const raw = {
      save_table_images: true,
      device: 'cpu' as const,
      dom_extraction: 'always' as const,
      image_quality: 90,
    };
    const normalized = normalizeConfig(raw);
    expect(normalized.save_table_images).toBe(true);
    expect(normalized.device).toBe('cpu');
    expect(normalized.dom_extraction).toBe('always');
    expect(normalized.image_quality).toBe(90);
  });

  it('normalizeConfig fills in missing fields from defaults safely', () => {
    const raw = {};
    const normalized = normalizeConfig(raw);
    expect(normalized.save_table_images).toBe(true);
    expect(normalized.device).toBe('auto');
    expect(normalized.dom_extraction).toBe('auto');
    expect(normalized.extract_tables).toBe(true);
  });

  it('getAssetUrl generates valid URLs in non-Tauri mode', () => {
    const url = getAssetUrl('/path/to/figure.png');
    expect(url).toContain('/api/asset?path=');
    expect(url).toContain(encodeURIComponent('/path/to/figure.png'));
  });
});
