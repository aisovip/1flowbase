import type { ReactNode } from 'react';
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor
} from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import type {
  NotificationInstance,
  NotificationType
} from '@1flowbase/api-client';
import { useAuthStore, resetAuthStore } from '../../../state/auth-store';
import { NotificationsPage } from '../../settings/pages/notifications/NotificationsPage';
import { NotificationBell } from '../components/NotificationBell';

class SignalStream {
  static instances: SignalStream[] = [];
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  refresh: (() => void) | null = null;
  close = vi.fn();
  constructor(
    readonly url: string,
    readonly options: EventSourceInit
  ) {
    SignalStream.instances.push(this);
  }
  addEventListener(_name: string, refresh: () => void) {
    this.refresh = refresh;
  }
}
const type: NotificationType = {
  type_code: 'system',
  display_name: '系统通知',
  description: null,
  config_schema: {},
  config_form: {
    schema_version: '1.0.0',
    fields: [
      {
        key: 'token',
        label: '访问令牌',
        type: 'string',
        control: 'password',
        required: true,
        options: [],
        visible_when: [],
        disabled_when: []
      }
    ]
  },
  input_schema: {},
  input_form: { schema_version: '1.0.0', fields: [] },
  output_schema: {},
  contribution_id: 'system',
  installation_id: null,
  available: true,
  unavailable_reason: null
};
const initialInstance: NotificationInstance = {
  id: 'instance-1',
  workspace_id: 'workspace-1',
  type_code: 'system',
  installation_id: null,
  display_name: '系统站内通知',
  status: 'enabled',
  config_json: {},
  configured_secret_fields: ['token'],
  available: true,
  unavailable_reason: null,
  created_at: '2026-10-13T00:00:00Z',
  updated_at: '2026-10-13T00:00:00Z'
};
let instance: NotificationInstance;
let catalog: NotificationType[];
let filteredInstances: NotificationInstance[] | undefined;
let read_at: string | null;
let rejectRead: boolean;
let rejectStatus: boolean;
let statusRequest: Promise<void> | undefined;
let calls: Array<{
  url: string;
  method: string;
  body?: Record<string, unknown>;
}>;
let client: QueryClient;
function renderUI(ui: ReactNode) {
  client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } }
  });
  return render(
    <QueryClientProvider client={client}>{ui}</QueryClientProvider>
  );
}
beforeEach(() => {
  instance = { ...initialInstance };
  catalog = [type];
  filteredInstances = undefined;
  read_at = null;
  rejectRead = false;
  rejectStatus = false;
  statusRequest = undefined;
  calls = [];
  SignalStream.instances = [];
  useAuthStore.setState({
    sessionStatus: 'authenticated',
    csrfToken: 'csrf',
    actor: {
      id: 'user-1',
      current_workspace_id: 'workspace-1',
      effective_display_role: 'root'
    } as NonNullable<ReturnType<typeof useAuthStore.getState>['actor']>
  });
  vi.stubGlobal('EventSource', SignalStream);
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      const method = init?.method ?? 'GET';
      const body = init?.body
        ? (JSON.parse(String(init.body)) as Record<string, unknown>)
        : undefined;
      calls.push({ url, method, body });
      const message = {
        id: 'message-1',
        workspace_id: 'workspace-1',
        instance_id: instance.id,
        recipient_user_id: 'user-1',
        title: '发布完成',
        content: '流程运行成功',
        read_at,
        created_at: instance.created_at
      };
      let data: unknown;
      if (url.endsWith('/types')) data = catalog;
      else if (url.endsWith('/instances') && method === 'POST') {
        instance = {
          ...instance,
          ...body,
          configured_secret_fields: []
        } as NotificationInstance;
        data = instance;
      } else if (url.includes('/notifications/instances') && method === 'GET')
        data = url.includes('?')
          ? (filteredInstances ?? [instance])
          : [instance];
      else if (url.includes('/instances/') && method === 'PATCH') {
        if (body?.status) {
          await statusRequest;
          if (rejectStatus)
            return new Response(
              JSON.stringify({
                error: { code: 'forbidden', message: 'Forbidden' }
              }),
              { status: 403, headers: { 'content-type': 'application/json' } }
            );
        }
        instance = { ...instance, ...body } as NotificationInstance;
        data = instance;
      } else if (url.endsWith('/unread-count'))
        data = { unread_count: read_at ? 0 : 1 };
      else if (url.endsWith('/read') || url.endsWith('/read-all')) {
        if (rejectRead)
          return new Response(
            JSON.stringify({
              error: { code: 'forbidden', message: 'Forbidden' }
            }),
            { status: 403, headers: { 'content-type': 'application/json' } }
          );
        read_at = '2026-10-13T01:00:00Z';
        data = url.endsWith('/read-all')
          ? { updated_count: 1 }
          : { ...message, read_at };
      } else if (url.includes('/messages?'))
        data = {
          items: [message],
          next_cursor: null,
          unread_count: read_at ? 0 : 1
        };
      else throw new Error(`Unexpected route ${url}`);
      return new Response(JSON.stringify({ data, meta: null }), {
        headers: { 'content-type': 'application/json' }
      });
    })
  );
});
afterEach(() => {
  client?.clear();
  resetAuthStore();
  vi.unstubAllGlobals();
});

describe('notification management and inbox AC-001/003/004/006', () => {
  test('creates only from available canonical notification types', async () => {
    catalog = [
      { ...type, config_form: { schema_version: '1.0.0', fields: [] } },
      {
        ...type,
        type_code: 'external',
        display_name: 'Unavailable external',
        available: false
      }
    ];
    renderUI(<NotificationsPage />);
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /新增/ })).toBeEnabled()
    );
    fireEvent.click(await screen.findByRole('button', { name: /新增/ }));
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(screen.queryByText('Unavailable external')).not.toBeInTheDocument();
    fireEvent.click(await screen.findByRole('menuitem', { name: '系统通知' }));
    expect(
      screen.queryByRole('combobox', { name: '通知类型' })
    ).not.toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: '通知类型' })).toHaveValue(
      '系统通知'
    );
    fireEvent.change(screen.getByRole('textbox', { name: '渠道名称' }), {
      target: { value: 'New notifications' }
    });
    fireEvent.click(screen.getByRole('button', { name: '提交' }));
    await screen.findByRole('button', { name: 'New notifications' });
    expect(calls.find((call) => call.method === 'POST')?.body).toEqual({
      type_code: 'system',
      installation_id: null,
      display_name: 'New notifications',
      config_json: {}
    });
  });
  test('keeps the selected installation identity when two installations contribute the same type', async () => {
    catalog = [
      {
        ...type,
        type_code: 'acme/mail',
        installation_id: 'installation-a',
        display_name: 'Mail A',
        config_form: { schema_version: '1.0.0', fields: [] }
      },
      {
        ...type,
        type_code: 'acme/mail',
        installation_id: 'installation-b',
        display_name: 'Mail B',
        config_form: { schema_version: '1.0.0', fields: [] }
      }
    ];
    renderUI(<NotificationsPage />);
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /新增/ })).toBeEnabled()
    );
    fireEvent.click(await screen.findByRole('button', { name: /新增/ }));
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Mail B' }));
    fireEvent.change(screen.getByRole('textbox', { name: '渠道名称' }), {
      target: { value: 'Team mail' }
    });
    fireEvent.click(screen.getByRole('button', { name: '提交' }));
    await screen.findByRole('button', { name: 'Team mail' });
    expect(calls.find((call) => call.method === 'POST')?.body).toMatchObject({
      type_code: 'acme/mail',
      installation_id: 'installation-b'
    });
  });
  test('applies canonical query conditions and renders backend results, then clears the conditions', async () => {
    renderUI(<NotificationsPage />);
    await screen.findByRole('button', { name: '系统站内通知' });
    fireEvent.click(screen.getByRole('button', { name: /筛选/ }));
    fireEvent.change(await screen.findByRole('textbox', { name: '渠道名称' }), {
      target: { value: 'does not exist' }
    });
    filteredInstances = [];
    fireEvent.click(screen.getByRole('button', { name: /^应\s*用$/u }));
    await screen.findByText('暂无通知渠道');
    const filtered = calls.find((call) => call.url.includes('display_name='));
    expect(
      new URL(filtered!.url, 'http://localhost').searchParams.get(
        'display_name'
      )
    ).toBe('does not exist');
    fireEvent.click(screen.getByRole('button', { name: /筛选/ }));
    fireEvent.click(await screen.findByRole('button', { name: /^清\s*除$/u }));
    await screen.findByRole('button', { name: '系统站内通知' });
    expect(screen.getByRole('button', { name: /筛选/ })).toHaveTextContent(
      '筛选'
    );
    expect(screen.getByRole('button', { name: /筛选/ })).not.toHaveTextContent(
      '(1)'
    );
  });
  test('edits the real settings drawer, preserves omitted secrets, and reloads saved names', async () => {
    renderUI(<NotificationsPage />);
    fireEvent.click(await screen.findByRole('button', { name: /^编\s*辑$/u }));
    const name = await screen.findByRole('textbox', { name: '渠道名称' });
    fireEvent.change(name, { target: { value: '团队通知' } });
    const token = document.querySelector('input[type="password"]');
    expect(token).toBeTruthy();
    expect(token).toHaveValue('');
    fireEvent.click(screen.getByRole('button', { name: '提交' }));
    await screen.findByRole('button', { name: '团队通知' });
    const patch = calls.find((call) => call.method === 'PATCH');
    expect(patch?.body).toEqual({ display_name: '团队通知', config_json: {} });
    fireEvent.click(screen.getByRole('button', { name: /^编\s*辑$/u }));
    expect(
      await screen.findByRole('textbox', { name: '渠道名称' })
    ).toHaveValue('团队通知');
  });
  test('disables the instance using its backend status and shows plugin unavailability without dropping it', async () => {
    instance.available = false;
    instance.unavailable_reason = 'plugin_unavailable';
    renderUI(<NotificationsPage />);
    await screen.findByText('通知插件不可用');
    const toggle = screen.getByRole('switch', { name: '系统站内通知: 状态' });
    expect(toggle).toBeChecked();
    expect(
      screen.queryByRole('button', { name: /^停\s*用$/u })
    ).not.toBeInTheDocument();
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).not.toBeChecked());
    expect(calls.find((call) => call.method === 'PATCH')?.body).toEqual({
      status: 'disabled'
    });
    expect(
      screen.getByRole('button', { name: '系统站内通知' })
    ).toBeInTheDocument();
    await waitFor(() => expect(toggle).toBeEnabled());
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toBeChecked());
    expect(
      calls.filter((call) => call.method === 'PATCH').at(-1)?.body
    ).toEqual({ status: 'enabled' });
  });
  test('keeps the backend switch state while saving and after a rejected status update', async () => {
    rejectStatus = true;
    let finishRequest!: () => void;
    statusRequest = new Promise<void>((resolve) => {
      finishRequest = resolve;
    });
    renderUI(<NotificationsPage />);
    const toggle = await screen.findByRole('switch', {
      name: '系统站内通知: 状态'
    });
    fireEvent.click(toggle);
    await waitFor(() => expect(toggle).toBeDisabled());
    expect(toggle).toBeChecked();
    expect(instance.status).toBe('enabled');
    finishRequest();
    await screen.findByText('通知实例操作失败，请刷新后重试。');
    expect(toggle).toBeChecked();
    expect(toggle).toBeEnabled();
    expect(instance.status).toBe('enabled');
  });
  test('marks messages read through HTTP and refetches after both a signal and reconnect', async () => {
    const view = renderUI(<NotificationBell />);
    fireEvent.click(screen.getByRole('button', { name: '通知消息' }));
    await screen.findByText('发布完成');
    fireEvent.click(screen.getByRole('button', { name: '标为已读' }));
    await screen.findByText('已读', { exact: true });
    const source = SignalStream.instances[0];
    expect(source.options).toEqual({ withCredentials: true });
    const before = calls.filter((call) =>
      call.url.includes('/messages?')
    ).length;
    act(() => {
      source.refresh?.();
    });
    await waitFor(() =>
      expect(
        calls.filter((call) => call.url.includes('/messages?')).length
      ).toBeGreaterThan(before)
    );
    const after = calls.filter((call) =>
      call.url.includes('/messages?')
    ).length;
    act(() => {
      source.onerror?.();
      source.onopen?.();
    });
    await waitFor(() =>
      expect(
        calls.filter((call) => call.url.includes('/messages?')).length
      ).toBeGreaterThan(after)
    );
    expect(screen.getByText('已读', { exact: true })).toBeInTheDocument();
    view.unmount();
    expect(source.close).toHaveBeenCalledOnce();
  });
  test('surfaces backend refusal without pretending the recipient read state changed', async () => {
    rejectRead = true;
    renderUI(<NotificationBell />);
    fireEvent.click(screen.getByRole('button', { name: '通知消息' }));
    fireEvent.click(await screen.findByRole('button', { name: '标为已读' }));
    await screen.findByText('消息操作失败，请刷新后重试。');
    expect(screen.getByText('未读', { exact: true })).toBeInTheDocument();
    expect(read_at).toBeNull();
    expect(calls.filter((call) => call.method === 'POST')[0].url).toContain(
      '/messages/message-1/read'
    );
  });
  test('marks all read and closes the previous actor connection when session scope changes', async () => {
    renderUI(<NotificationBell />);
    fireEvent.click(screen.getByRole('button', { name: '通知消息' }));
    await screen.findByText('发布完成');
    fireEvent.click(screen.getByRole('button', { name: '全部已读' }));
    await screen.findByText('已读', { exact: true });
    const source = SignalStream.instances[0];
    act(() =>
      useAuthStore.setState({
        actor: { ...useAuthStore.getState().actor!, id: 'user-2' }
      })
    );
    await waitFor(() => expect(source.close).toHaveBeenCalledOnce());
    expect(SignalStream.instances).toHaveLength(2);
  });
});
