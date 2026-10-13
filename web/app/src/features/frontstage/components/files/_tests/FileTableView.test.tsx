import '../../../../../test/fixtures/rc-util-unique-ids';
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { App } from 'antd';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import copy from 'copy-to-clipboard';
import * as client from '@1flowbase/api-client';
import { FileTableView } from '../FileTableView';
import { resetAuthStore, useAuthStore } from '../../../../../state/auth-store';

vi.mock('copy-to-clipboard', () => ({ default: vi.fn(() => true) }));

vi.mock('@1flowbase/api-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@1flowbase/api-client')>()),
  fetchConsoleFileTables: vi.fn(),
  fetchConsoleFileRecords: vi.fn(),
  uploadConsoleFile: vi.fn(),
  createConsoleTemporaryFileLink: vi.fn()
}));
const table = {
  id: 'table-1',
  code: 'file_demo',
  title: '文件表 Demo',
  status: 'active',
  scope_kind: 'workspace',
  scope_id: 'workspace-1',
  model_definition_id: 'model-1',
  bound_storage_id: null,
  bound_storage_title: null,
  is_builtin: false,
  is_default: false
} as const;
const record = {
  id: 'record-1',
  url: '/api/console/files/table-1/records/record-1/content',
  filename: 'hello.txt',
  title: 'hello.txt',
  mimetype: 'text/plain',
  size: 5,
  storage_id: 'storage-1',
  created_at: '2026-10-12T12:00:00Z'
};

function mount(
  queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } }
  })
) {
  return render(
    <QueryClientProvider client={queryClient}>
      <App>
        <FileTableView fileTableCode="file_demo" />
      </App>
    </QueryClientProvider>
  );
}
beforeEach(() => {
  vi.clearAllMocks();
  useAuthStore.setState({
    csrfToken: 'csrf-123',
    actor: {
      id: 'user-1',
      account: 'root',
      effective_display_role: 'root',
      current_workspace_id: 'workspace-1'
    }
  });
  vi.mocked(client.fetchConsoleFileTables).mockResolvedValue([table]);
  vi.mocked(client.fetchConsoleFileRecords).mockResolvedValue({
    items: [],
    total: 0
  });
});
afterEach(() => {
  cleanup();
  resetAuthStore();
});

describe('FileTableView', () => {
  test('selects the requested file table, uploads multipart through the host and refreshes records', async () => {
    vi.mocked(client.uploadConsoleFile).mockImplementation(async () => {
      vi.mocked(client.fetchConsoleFileRecords).mockResolvedValue({
        items: [record],
        total: 1
      });
      return { file_table_id: table.id, storage_id: record.storage_id, record };
    });
    mount();
    const button = await screen.findByRole('button', { name: /上传文件/ });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    const dialog = await screen.findByRole('dialog', { name: '上传文件' });
    const input = dialog.querySelector('input[type="file"]')!;
    const file = new File(['hello'], 'hello.txt', { type: 'text/plain' });
    fireEvent.change(input, { target: { files: [file] } });
    await waitFor(() =>
      expect(client.uploadConsoleFile).toHaveBeenCalledWith(
        expect.any(File),
        table.id,
        'csrf-123',
        expect.any(AbortSignal)
      )
    );
    expect(
      await screen.findByRole('link', { name: 'hello.txt' })
    ).toHaveAttribute(
      'href',
      '/api/console/files/table-1/records/record-1/content'
    );
    expect(client.fetchConsoleFileRecords).toHaveBeenCalledWith(
      'file_demo',
      1,
      20,
      expect.any(AbortSignal)
    );
  });
  test('does not enable upload or query records for a missing target table', async () => {
    vi.mocked(client.fetchConsoleFileTables).mockResolvedValue([]);
    mount();
    expect(
      await screen.findByText('文件表不可用，请检查文件表配置和访问权限。')
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /上传文件/ })).toBeDisabled();
    expect(client.fetchConsoleFileRecords).not.toHaveBeenCalled();
  });
  test('a rejected upload does not add a row or refetch records as a success', async () => {
    vi.mocked(client.uploadConsoleFile).mockRejectedValue(
      new Error('存储不可用')
    );
    mount();
    const button = await screen.findByRole('button', { name: /上传文件/ });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    const dialog = await screen.findByRole('dialog', { name: '上传文件' });
    fireEvent.change(dialog.querySelector('input[type="file"]')!, {
      target: { files: [new File(['hello'], 'hello.txt')] }
    });
    expect(await screen.findByText('存储不可用')).toBeInTheDocument();
    expect(client.fetchConsoleFileRecords).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByRole('link', { name: 'hello.txt' })
    ).not.toBeInTheDocument();
  });

  test('uploading from another page refreshes a still-fresh cached first page', async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, staleTime: Infinity } }
    });
    queryClient.setQueryData(['files', 'tables', 'workspace-1'], [table]);
    queryClient.setQueryData(['files', 'records', 'workspace-1', table.id, 1], {
      items: [record],
      total: 21
    });
    const second = { ...record, id: 'record-2', filename: 'second.txt' };
    const newest = { ...record, id: 'record-3', filename: 'newest.txt' };
    let uploaded = false;
    vi.mocked(client.fetchConsoleFileRecords).mockImplementation(
      async (_, page) => ({
        items: page === 1 ? [uploaded ? newest : record] : [second],
        total: 21
      })
    );
    vi.mocked(client.uploadConsoleFile).mockImplementation(async () => {
      uploaded = true;
      return {
        file_table_id: table.id,
        storage_id: record.storage_id,
        record: newest
      };
    });
    mount(queryClient);
    fireEvent.click(screen.getByTitle('2'));
    await screen.findByRole('link', { name: 'second.txt' });
    fireEvent.click(screen.getByRole('button', { name: /上传文件/ }));
    const dialog = await screen.findByRole('dialog', { name: '上传文件' });
    fireEvent.change(dialog.querySelector('input[type="file"]')!, {
      target: { files: [new File(['newest'], 'newest.txt')] }
    });
    expect(
      await screen.findByRole('link', { name: 'newest.txt' })
    ).toBeInTheDocument();
  });
  test('uses the persisted path and creates and copies a temporary link', async () => {
    const stored = { ...record, url: '/files/persisted/hello.txt' };
    vi.mocked(client.fetchConsoleFileRecords).mockResolvedValue({
      items: [stored],
      total: 1
    });
    vi.mocked(client.createConsoleTemporaryFileLink).mockResolvedValue({
      url: '/api/public/files/content?token=temporary',
      expires_at: '2026-10-13T12:00:00Z'
    });
    mount();
    expect(
      await screen.findByRole('link', { name: 'hello.txt' })
    ).toHaveAttribute('href', stored.url);
    fireEvent.click(await screen.findByRole('button', { name: '临时公开' }));
    await waitFor(() =>
      expect(
        screen.getByText('有效期内，任何持有链接的人均可访问此文件。')
      ).toBeVisible()
    );
    fireEvent.change(screen.getByRole('spinbutton'), {
      target: { value: '120' }
    });
    fireEvent.click(screen.getByRole('button', { name: '生成链接' }));
    const field = await screen.findByRole('textbox', { name: '临时访问链接' });
    const url = new URL(
      '/api/public/files/content?token=temporary',
      window.location.origin
    ).href;
    expect(field).toHaveValue(url);
    expect(client.createConsoleTemporaryFileLink).toHaveBeenCalledWith(
      table.id,
      record.id,
      { expires_in_seconds: 120 },
      'csrf-123',
      expect.any(AbortSignal)
    );
    fireEvent.click(screen.getByRole('button', { name: '复制链接' }));
    expect(copy).toHaveBeenCalledWith(url);
    expect(screen.getByText(/到期时间：/)).toBeVisible();
  });

  test('closes a pending link request on workspace change and ignores its late result', async () => {
    vi.mocked(client.fetchConsoleFileRecords).mockResolvedValue({
      items: [record],
      total: 1
    });
    let resolve!: (value: client.TemporaryFileLink) => void;
    vi.mocked(client.createConsoleTemporaryFileLink).mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        })
    );
    mount();
    fireEvent.click(await screen.findByRole('button', { name: '临时公开' }));
    fireEvent.click(screen.getByRole('button', { name: '生成链接' }));
    await waitFor(() =>
      expect(client.createConsoleTemporaryFileLink).toHaveBeenCalled()
    );
    const signal = vi.mocked(client.createConsoleTemporaryFileLink).mock
      .calls[0][4]!;
    useAuthStore.setState({
      actor: {
        id: 'user-1',
        account: 'root',
        effective_display_role: 'root',
        current_workspace_id: 'workspace-2'
      }
    });
    await waitFor(() => expect(signal.aborted).toBe(true));
    resolve({
      url: '/api/public/files/content?token=stale',
      expires_at: '2026-10-13T12:00:00Z'
    });
    await waitFor(() =>
      expect(
        screen.queryByRole('textbox', { name: '临时访问链接' })
      ).not.toBeInTheDocument()
    );
  });

  test('reports a failed link request without displaying a stale link', async () => {
    vi.mocked(client.fetchConsoleFileRecords).mockResolvedValue({
      items: [record],
      total: 1
    });
    vi.mocked(client.createConsoleTemporaryFileLink).mockRejectedValue(
      new Error('file access denied')
    );
    mount();
    fireEvent.click(await screen.findByRole('button', { name: '临时公开' }));
    fireEvent.click(screen.getByRole('button', { name: '生成链接' }));
    await waitFor(() =>
      expect(screen.getByText('file access denied')).toBeVisible()
    );
    expect(
      screen.queryByRole('textbox', { name: '临时访问链接' })
    ).not.toBeInTheDocument();
  });
});
