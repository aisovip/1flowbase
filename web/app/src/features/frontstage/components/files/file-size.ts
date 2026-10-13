import { formatNumber } from '../../../../shared/i18n/format';

const BYTE_UNITS = [
  'B',
  'KiB',
  'MiB',
  'GiB',
  'TiB',
  'PiB',
  'EiB',
  'ZiB',
  'YiB'
];

export function formatFileSize(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${formatNumber(value, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2
  })} ${BYTE_UNITS[unit]}`;
}
