import { sanitizeUrl } from './utils';

describe('sanitizeUrl', () => {
  it('should allow http URLs', () => {
    expect(sanitizeUrl('http://example.com')).toBe('http://example.com');
    expect(sanitizeUrl('http://example.com/path')).toBe('http://example.com/path');
  });

  it('should allow https URLs', () => {
    expect(sanitizeUrl('https://example.com')).toBe('https://example.com');
    expect(sanitizeUrl('https://example.com/path')).toBe('https://example.com/path');
  });

  it('should reject javascript: URLs', () => {
    // eslint-disable-next-line no-script-url
    expect(sanitizeUrl('javascript:alert(1)')).toBeUndefined();
  });

  it('should reject data: URLs', () => {
    expect(sanitizeUrl('data:text/html,<script>alert(1)</script>')).toBeUndefined();
  });

  it('should reject file: URLs', () => {
    expect(sanitizeUrl('file:///etc/passwd')).toBeUndefined();
  });

  it('should reject vbscript: URLs', () => {
    expect(sanitizeUrl('vbscript:msgbox(1)')).toBeUndefined();
  });

  it('should handle undefined input', () => {
    expect(sanitizeUrl(undefined)).toBeUndefined();
  });

  it('should handle empty string', () => {
    expect(sanitizeUrl('')).toBeUndefined();
    expect(sanitizeUrl('   ')).toBeUndefined();
  });

  it('should handle invalid URLs', () => {
    expect(sanitizeUrl('not a url')).toBeUndefined();
    expect(sanitizeUrl('htp://example.com')).toBeUndefined();
  });

  it('should preserve query parameters', () => {
    expect(sanitizeUrl('https://example.com?foo=bar')).toBe('https://example.com?foo=bar');
  });

  it('should preserve URL fragments', () => {
    expect(sanitizeUrl('https://example.com#section')).toBe('https://example.com#section');
  });
});
