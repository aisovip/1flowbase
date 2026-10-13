import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { NotificationLogs } from '../NotificationLogs';
import { resetAuthStore, useAuthStore } from '../../../../../state/auth-store';

let client: QueryClient;
let calls: string[];
const log = {
  id: 'log-1',
  workspace_id: 'workspace-1',
  actor_user_id: 'user-1',
  instance_id: 'channel-1',
  display_name: 'Delivery channel',
  type_code: 'system',
  source: 'workflow',
  status: 'failure',
  failure_code: 'input_invalid',
  title: 'Plain title',
  content: '<script>raw text</script>',
  recipient_count: 2,
  created_at: '2026-10-13T00:00:00Z',
  completed_at: null
};
beforeEach(() => {
  calls = [];
  useAuthStore.setState({
    sessionStatus: 'authenticated',
    csrfToken: 'csrf',
    actor: {
      id: 'user-1',
      current_workspace_id: 'workspace-1',
      effective_display_role: 'root'
    } as NonNullable<ReturnType<typeof useAuthStore.getState>['actor']>
  });
  vi.stubGlobal(
    'fetch',
    vi.fn(async (input: string) => {
      calls.push(input);
      const url = new URL(input, 'http://localhost');
      const data = url.pathname.endsWith('/types')
        ? []
        : url.pathname.endsWith('/instances')
          ? []
          : url.pathname.endsWith('/logs/log-1')
            ? log
            : {
                items: [
                  {
                    ...log,
                    id:
                      url.searchParams.get('page') === '2' ? 'log-2' : 'log-1',
                    display_name:
                      url.searchParams.get('page') === '2'
                        ? 'Second page channel'
                        : log.display_name
                  }
                ],
                page: Number(url.searchParams.get('page') ?? 1),
                page_size: 20,
                total: 21
              };
      return new Response(JSON.stringify({ data, meta: null }), {
        headers: { 'content-type': 'application/json' }
      });
    })
  );
  client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <NotificationLogs />
    </QueryClientProvider>
  );
});
afterEach(() => {
  client.clear();
  resetAuthStore();
  vi.unstubAllGlobals();
});
test('uses backend pages and exposes only safe, plain-text log detail', async () => {
  await screen.findByText('Delivery channel');
  fireEvent.click(screen.getByRole('button', { name: '详情' }));
  await screen.findByText('Plain title');
  expect(screen.getByText('<script>raw text</script>')).toBeInTheDocument();
  expect(document.querySelector('.notification-log-detail script')).toBeNull();
  expect(screen.getAllByText('消息输入或收件人无效').length).toBeGreaterThan(0);
  expect(screen.queryByText('input_invalid')).not.toBeInTheDocument();
  expect(
    screen.queryByRole('button', { name: /重试|删除/ })
  ).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Close' }));
  fireEvent.click(screen.getByTitle('2'));
  await screen.findByText('Second page channel');
  expect(
    calls.some(
      (url) => new URL(url, 'http://localhost').searchParams.get('page') === '2'
    )
  ).toBe(true);
});
test('sends source filter to the server and resets the page', async () => {
  await screen.findByText('Delivery channel');
  fireEvent.mouseDown(screen.getByRole('combobox', { name: '触发来源' }));
  fireEvent.click(await screen.findByText('API 调用'));
  await waitFor(() =>
    expect(
      calls.some((input) => {
        const params = new URL(input, 'http://localhost').searchParams;
        return params.get('source') === 'api' && params.get('page') === '1';
      })
    ).toBe(true)
  );
});
