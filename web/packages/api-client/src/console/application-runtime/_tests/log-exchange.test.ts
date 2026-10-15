import { beforeEach, expect, test, vi } from 'vitest';
import { apiFetch, apiFetchBlob } from '../../../transport';
import {
  exportConsoleApplicationLogsOtlp,
  importConsoleApplicationLogsOtlp
} from '../log-exchange';
vi.mock('../../../transport', () => ({
  apiFetch: vi.fn(),
  apiFetchBlob: vi.fn()
}));
beforeEach(() => vi.clearAllMocks());
test('OTLP exchange posts record IDs and OTLP object through CSRF transport without run aliases', async () => {
  await exportConsoleApplicationLogsOtlp(
    'app',
    { record_ids: ['native', 'imported'] },
    'csrf',
    'https://api.example'
  );
  expect(apiFetchBlob).toHaveBeenCalledWith({
    path: '/api/console/applications/app/logs/otlp/export',
    method: 'POST',
    body: { record_ids: ['native', 'imported'] },
    csrfToken: 'csrf',
    baseUrl: 'https://api.example'
  });
  const otlp = { resourceSpans: [] };
  await importConsoleApplicationLogsOtlp('app', { otlp }, 'csrf');
  expect(apiFetch).toHaveBeenCalledWith({
    path: '/api/console/applications/app/logs/otlp/import',
    method: 'POST',
    body: { otlp },
    csrfToken: 'csrf',
    baseUrl: undefined
  });
});
test('conversation selectors preserve canonical IDs through the standard export transport', async () => {
  const selection = {
    log_conversation_ids: ['conversation-one', 'conversation-two']
  };
  await exportConsoleApplicationLogsOtlp('app', selection, 'csrf');
  expect(apiFetchBlob).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/otlp/export',
    method: 'POST',
    body: selection,
    csrfToken: 'csrf',
    baseUrl: undefined
  });
});
