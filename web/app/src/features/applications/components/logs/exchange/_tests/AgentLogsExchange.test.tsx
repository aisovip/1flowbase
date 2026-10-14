import '../../../../../../test/fixtures/rc-util-unique-ids';
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within
} from '@testing-library/react';
import { beforeEach, expect, test, vi } from 'vitest';
import { appI18n } from '../../../../../../shared/i18n/app-i18n';
import { useAuthStore } from '../../../../../../state/auth-store';
import { AgentLogsExchange } from '../AgentLogsExchange';
const api = vi.hoisted(() => ({
  exportApplicationLogsOtlp: vi.fn(),
  importApplicationLogsOtlp: vi.fn(),
  exportApplicationLogsLangfuse: vi.fn(),
  importApplicationLogsLangfuse: vi.fn()
}));
const save = vi.hoisted(() => vi.fn());
vi.mock('../../../../api/log-exchange', () => api);
vi.mock('../../../../lib/run-export-download', () => ({
  saveApplicationRunExport: save
}));
beforeEach(async () => {
  for (const fn of Object.values(api)) fn.mockReset();
  save.mockReset();
  await appI18n.changeLanguage('zh_Hans');
  useAuthStore.setState({ csrfToken: 'fixture-csrf' });
});
function open(
  selectedRecords = [
    { id: 'r1', application_id: 'app-a' },
    { id: 'r2', application_id: 'app-b' }
  ],
  applicationId = ''
) {
  const onImported = vi.fn();
  render(
    <AgentLogsExchange
      applicationId={applicationId}
      applications={[
        { id: 'app-a', name: 'App A' },
        { id: 'app-b', name: 'App B' }
      ]}
      selectedRecords={selectedRecords}
      onImported={onImported}
    />
  );
  fireEvent.click(screen.getByRole('button', { name: '日志交换' }));
  return onImported;
}
async function operation(label: string) {
  fireEvent.mouseDown(screen.getByRole('combobox', { name: '交换方式' }));
  fireEvent.click(
    await screen.findByText(label, {
      selector: '.ant-select-item-option-content'
    })
  );
}
function credentials() {
  fireEvent.change(screen.getByLabelText('Langfuse 地址'), {
    target: { value: 'https://langfuse.example' }
  });
  fireEvent.change(screen.getByLabelText('Public Key'), {
    target: { value: 'public-fixture' }
  });
  fireEvent.change(screen.getByLabelText('Secret Key'), {
    target: { value: 'secret-fixture' }
  });
}
function execute() {
  fireEvent.click(screen.getByRole('button', { name: '执行交换' }));
}

test('OTLP export groups the selection by owner and preserves a successful download when another app fails', async () => {
  api.exportApplicationLogsOtlp
    .mockResolvedValueOnce({
      blob: new Blob(['{}']),
      filename: 'server-otlp.json',
      contentType: 'application/json'
    })
    .mockRejectedValueOnce(new Error('connection'));
  open();
  execute();
  expect(
    await screen.findByText('已下载 server-otlp.json')
  ).toBeInTheDocument();
  expect(
    await screen.findByText('交换失败，请检查输入或连接后重试。')
  ).toBeInTheDocument();
  expect(api.exportApplicationLogsOtlp.mock.calls).toEqual([
    ['app-a', ['r1'], 'fixture-csrf'],
    ['app-b', ['r2'], 'fixture-csrf']
  ]);
  expect(save).toHaveBeenCalledOnce();
});

test('Langfuse multi-record export prevents reentry and exposes partial receipt without success text', async () => {
  let resolve!: (value: unknown) => void;
  api.exportApplicationLogsLangfuse.mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done;
      })
  );
  open(
    [
      { id: 'r1', application_id: 'app-a' },
      { id: 'r2', application_id: 'app-a' }
    ],
    'app-a'
  );
  await operation('导出到 Langfuse');
  credentials();
  execute();
  execute();
  expect(api.exportApplicationLogsLangfuse).toHaveBeenCalledOnce();
  expect(screen.getByRole('button', { name: '关闭' })).toBeDisabled();
  resolve({ exported_records: 1, exported_spans: 4, trace_ids: ['trace-one'] });
  expect(
    await screen.findByText('服务端仅导出了部分所选日志，请核对回执。')
  ).toBeInTheDocument();
  expect(screen.getByText('trace-one')).toBeInTheDocument();
  expect(api.exportApplicationLogsLangfuse.mock.calls[0]).toEqual([
    'app-a',
    ['r1', 'r2'],
    {
      base_url: 'https://langfuse.example',
      public_key: 'public-fixture',
      secret_key: 'secret-fixture'
    },
    'fixture-csrf'
  ]);
});

test('global Langfuse import requires explicit destination, sends multiple IDs and clears credentials on close', async () => {
  api.importApplicationLogsLangfuse.mockResolvedValue({
    accepted_events: 3,
    duplicate_events: 2,
    record_ids: ['imported-one', 'imported-two']
  });
  const onImported = open([]);
  await operation('从 Langfuse 导入');
  credentials();
  expect(screen.getByLabelText('Secret Key')).toHaveAttribute(
    'type',
    'password'
  );
  fireEvent.change(screen.getByLabelText('Trace IDs（逗号或换行分隔）'), {
    target: { value: 'trace-1,trace-2\ntrace-1' }
  });
  expect(screen.getByRole('button', { name: '执行交换' })).toBeDisabled();
  fireEvent.mouseDown(
    screen.getByRole('combobox', { name: '选择导入目标应用' })
  );
  fireEvent.click(
    await screen.findByText('App B', {
      selector: '.ant-select-item-option-content'
    })
  );
  execute();
  expect(
    await screen.findByText('imported-one, imported-two')
  ).toBeInTheDocument();
  expect(screen.getByText('重复事件数')).toBeInTheDocument();
  expect(api.importApplicationLogsLangfuse).toHaveBeenCalledWith(
    'app-b',
    ['trace-1', 'trace-2'],
    {
      base_url: 'https://langfuse.example',
      public_key: 'public-fixture',
      secret_key: 'secret-fixture'
    },
    'fixture-csrf'
  );
  expect(onImported).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole('button', { name: '关闭' }));
  await waitFor(() =>
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  );
  fireEvent.click(screen.getByRole('button', { name: '日志交换' }));
  await operation('从 Langfuse 导入');
  expect(screen.getByLabelText('Secret Key')).toHaveValue('');
  expect(screen.getByLabelText('Public Key')).toHaveValue('');
  expect(screen.getByLabelText('Langfuse 地址')).toHaveValue('');
});

test('OTLP file import uses the object unchanged and shows actual duplicate receipt', async () => {
  const otlp = { resourceSpans: [{ scopeSpans: [] }] };
  api.importApplicationLogsOtlp.mockResolvedValue({
    accepted_events: 0,
    duplicate_events: 7,
    record_ids: ['existing-record']
  });
  open([], 'app-a');
  const file = new File(['unused'], 'logs.json', { type: 'application/json' });
  Object.defineProperty(file, 'text', {
    value: async () => JSON.stringify(otlp)
  });
  fireEvent.change(screen.getByLabelText('OTLP JSON 文件'), {
    target: { files: [file] }
  });
  await waitFor(() =>
    expect(screen.getByRole('button', { name: '执行交换' })).toBeEnabled()
  );
  execute();
  expect(await screen.findByText('existing-record')).toBeInTheDocument();
  expect(api.importApplicationLogsOtlp).toHaveBeenCalledWith(
    'app-a',
    otlp,
    'fixture-csrf'
  );
  expect(within(screen.getByRole('dialog')).getByText('7')).toBeInTheDocument();
});

test('invalid OTLP file and missing CSRF cannot start exchange', async () => {
  useAuthStore.setState({ csrfToken: null });
  open([], 'app-a');
  const file = new File(['unused'], 'invalid.json');
  Object.defineProperty(file, 'text', { value: async () => '[]' });
  fireEvent.change(screen.getByLabelText('OTLP JSON 文件'), {
    target: { files: [file] }
  });
  expect(
    await screen.findByText('请选择有效的 JSON 对象文件。')
  ).toBeInTheDocument();
  expect(screen.getByRole('button', { name: '执行交换' })).toBeDisabled();
  expect(api.importApplicationLogsOtlp).not.toHaveBeenCalled();
});
