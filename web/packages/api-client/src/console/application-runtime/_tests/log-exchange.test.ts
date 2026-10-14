import { beforeEach, expect, test, vi } from 'vitest';
import { apiFetch, apiFetchBlob } from '../../../transport';
import {
  exportConsoleApplicationLogsOtlp,
  importConsoleApplicationLogsOtlp,
  exportConsoleApplicationLogsLangfuse,
  importConsoleApplicationLogsLangfuse
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
test('Langfuse exchange preserves exact credential fields and multiple record or trace IDs', async () => {
  const credentials = {
    base_url: 'https://langfuse.example',
    public_key: 'public',
    secret_key: 'secret'
  };
  await exportConsoleApplicationLogsLangfuse(
    'app',
    { ...credentials, record_ids: ['r1', 'r2'] },
    'csrf'
  );
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/langfuse/export',
    method: 'POST',
    body: { ...credentials, record_ids: ['r1', 'r2'] },
    csrfToken: 'csrf',
    baseUrl: undefined
  });
  await importConsoleApplicationLogsLangfuse(
    'app',
    { ...credentials, trace_ids: ['t1', 't2'] },
    'csrf'
  );
  expect(apiFetch).toHaveBeenLastCalledWith({
    path: '/api/console/applications/app/logs/langfuse/import',
    method: 'POST',
    body: { ...credentials, trace_ids: ['t1', 't2'] },
    csrfToken: 'csrf',
    baseUrl: undefined
  });
});
