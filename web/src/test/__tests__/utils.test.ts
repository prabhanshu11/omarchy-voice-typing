import { describe, it, expect } from 'vitest';
import { formatBytes, formatDate, formatDuration, formatMs } from '../../lib/utils';

describe('formatBytes', () => {
  it('formats bytes', () => {
    expect(formatBytes(500)).toBe('500 B');
  });

  it('formats kilobytes', () => {
    expect(formatBytes(2048)).toBe('2.0 KB');
  });

  it('formats megabytes', () => {
    expect(formatBytes(1048576)).toBe('1.00 MB');
  });

  it('formats zero', () => {
    expect(formatBytes(0)).toBe('0 B');
  });
});

describe('formatDate', () => {
  it('formats valid timestamp', () => {
    const result = formatDate('2026-02-20T14:30:00');
    expect(result).toContain('Feb');
    expect(result).toContain('20');
  });

  it('returns Unknown for empty string', () => {
    expect(formatDate('')).toBe('Unknown');
  });

  it('returns Unknown for zero date', () => {
    expect(formatDate('0001-01-01T00:00:00Z')).toBe('Unknown');
  });
});

describe('formatDuration', () => {
  it('formats seconds', () => {
    expect(formatDuration(45)).toBe('0:45');
  });

  it('formats minutes and seconds', () => {
    expect(formatDuration(125)).toBe('2:05');
  });

  it('formats zero', () => {
    expect(formatDuration(0)).toBe('0:00');
  });
});

describe('formatMs', () => {
  it('formats milliseconds', () => {
    expect(formatMs(500)).toBe('500ms');
  });

  it('formats seconds', () => {
    expect(formatMs(1500)).toBe('1.5s');
  });

  it('returns N/A for negative', () => {
    expect(formatMs(-1)).toBe('N/A');
  });

  it('formats zero', () => {
    expect(formatMs(0)).toBe('0ms');
  });
});
