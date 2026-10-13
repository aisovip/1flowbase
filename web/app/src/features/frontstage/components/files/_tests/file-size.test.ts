import { describe, expect, test } from 'vitest';
import { formatFileSize } from '../file-size';

describe('file size presentation', () => {
  test.each([
    [0, '0.00 B'],
    [31, '31.00 B'],
    [1023, '1,023.00 B'],
    [1024, '1.00 KiB'],
    [2924, '2.86 KiB'],
    [1_667_307, '1.59 MiB'],
    [1024 ** 3, '1.00 GiB'],
    [1024 ** 4, '1.00 TiB']
  ])('formats %i bytes as %s', (bytes, expected) => {
    expect(formatFileSize(bytes as number)).toBe(expected);
  });
});
