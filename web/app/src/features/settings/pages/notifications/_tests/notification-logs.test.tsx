import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { NotificationSendLog } from '@1flowbase/api-client';
import { appI18n } from '../../../../../shared/i18n/app-i18n';
import { NotificationLogs } from '../NotificationLogs';
import { resetAuthStore, useAuthStore } from '../../../../../state/auth-store';

let client: QueryClient;
let calls: string[];
const log: NotificationSendLog = {
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
let detail: NotificationSendLog;
beforeEach(() => {
  detail = { ...log };
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
            ? detail
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
afterEach(async () => {
  await appI18n.changeLanguage('zh_Hans');
  client.clear();
  resetAuthStore();
  vi.unstubAllGlobals();
});
test('uses backend pages and exposes only safe, plain-text log detail', async () => {
  await screen.findByText('Delivery channel');
  fireEvent.click(screen.getByRole('button', { name: '详情' }));
  await screen.findByText('Plain title');
  expect(screen.getByRole('textbox', { name: '日志 ID' })).toHaveValue('log-1');
  expect(screen.getByRole('textbox', { name: '日志 ID' })).toHaveAttribute(
    'readonly'
  );
  expect(screen.getByText('channel-1')).toBeInTheDocument();
  expect(screen.getByText('站内信')).toBeInTheDocument();
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

test('hides failure reasons for successful delivery and retains a safe reason for an unknown result', async () => {
  await screen.findByText('Delivery channel');
  detail = { ...log, status: 'success', failure_code: 'input_invalid' };
  fireEvent.click(screen.getByRole('button', { name: '详情' }));
  await screen.findByText('Plain title');
  const success = within(screen.getByRole('dialog'));
  expect(success.getByText('发送成功')).toBeInTheDocument();
  expect(success.queryByText('失败原因')).not.toBeInTheDocument();
  expect(success.queryByText('消息输入或收件人无效')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Close' }));
  detail = {
    ...log,
    status: 'unknown',
    failure_code: 'provider_result_unknown'
  };
  await client.invalidateQueries({ queryKey: ['notifications'] });
  fireEvent.click(screen.getByRole('button', { name: '详情' }));
  await screen.findByText('供应商发送结果未确认');
  const unknown = within(screen.getByRole('dialog'));
  expect(unknown.getByText('结果待确认')).toBeInTheDocument();
  expect(unknown.getByText('失败原因')).toBeInTheDocument();
  expect(
    unknown.queryByText('provider_result_unknown')
  ).not.toBeInTheDocument();
});

test.each([
  [
    'authorization_required',
    '渠道尚未授权，请先完成扫码授权',
    'This channel needs authorization before sending'
  ],
  [
    'authorization_invalid',
    '渠道授权已失效，请重新扫码授权',
    'Channel authorization is invalid. Authorize again'
  ],
  [
    'recipient_not_ready',
    '收件人会话尚未就绪，请向机器人发送一条消息后重试',
    'The recipient conversation is not ready. Send a message to the bot and try again'
  ],
  [
    'provider_rejected',
    '供应商拒绝了本次消息发送',
    'The provider rejected the message'
  ],
  [
    'provider_result_unknown',
    '供应商发送结果未确认',
    'The provider delivery result is unconfirmed'
  ]
])(
  'renders safe translated reason for %s without provider diagnostics',
  async (failure_code, label, english) => {
    await screen.findByText('Delivery channel');
    detail = {
      ...log,
      failure_code,
      status:
        failure_code === 'provider_result_unknown' ? 'unknown' : 'failure',
      // An unexpected extra field must never become a default UI diagnostic.
      ...{ provider_error: 'SECRET_TOKEN raw provider exception' }
    };
    fireEvent.click(screen.getByRole('button', { name: '详情' }));
    await screen.findByText(label);
    const drawer = within(screen.getByRole('dialog'));
    expect(drawer.getByText(label)).toBeInTheDocument();
    expect(drawer.queryByText(failure_code)).not.toBeInTheDocument();
    expect(
      drawer.queryByText(/SECRET_TOKEN|raw provider exception/)
    ).not.toBeInTheDocument();
    await act(async () => {
      await appI18n.changeLanguage('en_US');
    });
    expect(drawer.getByText(english)).toBeInTheDocument();
  }
);
