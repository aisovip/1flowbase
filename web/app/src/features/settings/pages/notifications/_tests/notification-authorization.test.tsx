import {
  act,
  fireEvent,
  render,
  screen,
  waitFor
} from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type {
  NotificationAuthorizationView,
  NotificationInstance,
  NotificationType
} from '@1flowbase/api-client';
import { NotificationInstanceDrawer } from '../NotificationInstanceDrawer';
import { resetAuthStore, useAuthStore } from '../../../../../state/auth-store';

const type: NotificationType = {
  type_code: 'vendor/private-notification',
  display_name: '扫码渠道',
  description: null,
  config_schema: {},
  config_form: {
    schema_version: '1.0.0',
    fields: [
      {
        key: 'endpoint',
        label: '服务地址',
        type: 'string',
        required: false,
        options: [],
        visible_when: [],
        disabled_when: []
      }
    ]
  },
  input_schema: {},
  input_form: { schema_version: '1.0.0', fields: [] },
  output_schema: {},
  authorization: {
    actions: [{ code: 'scan', label: '扫码授权', kind: 'qr_code' }],
    managed_secret_keys: ['bot_token']
  },
  contribution_id: 'notification',
  installation_id: 'installation-1',
  available: true,
  unavailable_reason: null
};
const instance: NotificationInstance = {
  id: 'instance-1',
  workspace_id: 'workspace-1',
  type_code: type.type_code,
  installation_id: type.installation_id,
  display_name: '我的渠道',
  status: 'disabled',
  config_json: { endpoint: 'https://example.com' },
  configured_secret_fields: [],
  authorization: { status: 'unauthorized' },
  available: false,
  unavailable_reason: 'instance_disabled',
  created_at: '2026-10-15T00:00:00Z',
  updated_at: '2026-10-15T00:00:00Z'
};
const pending: NotificationAuthorizationView = {
  status: 'pending',
  user_action: {
    kind: 'qr_code',
    qr_code_url: 'https://example.com/qr',
    poll_interval_seconds: 5
  }
};
let commands: Array<{
  path: string;
  body: Record<string, unknown>;
  signal?: AbortSignal | null;
}>;
let poll: () => Promise<NotificationAuthorizationView>;
let begin: NotificationAuthorizationView;
const saved = vi.fn();
function respond(data: unknown) {
  return new Response(JSON.stringify({ data, meta: null }), {
    headers: { 'content-type': 'application/json' }
  });
}
beforeEach(() => {
  commands = [];
  poll = async () => ({ status: 'authorized' });
  begin = pending;
  saved.mockReset();
  useAuthStore.setState({ csrfToken: 'csrf' });
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, init?: RequestInit) => {
      const body = JSON.parse(String(init?.body ?? '{}')) as Record<
        string,
        unknown
      >;
      const path = new URL(url, 'http://localhost').pathname;
      commands.push({ path, body, signal: init?.signal });
      if (path.endsWith('/authorization')) {
        if (body.type === 'begin') return respond(begin);
        if (body.type === 'poll') return respond(await poll());
        if (body.type === 'cancel') return respond({ status: 'unauthorized' });
      }
      return respond({
        ...instance,
        ...body,
        id: 'persisted-id',
        authorization: { status: 'unauthorized' }
      });
    })
  );
});
afterEach(() => {
  vi.useRealTimers();
  resetAuthStore();
  vi.unstubAllGlobals();
});

test('persists a new instance before metadata-driven QR, retains edits and polls to authorized', async () => {
  vi.useFakeTimers();
  render(
    <NotificationInstanceDrawer type={type} onClose={vi.fn()} onSaved={saved} />
  );
  fireEvent.change(screen.getByRole('textbox', { name: '渠道名称' }), {
    target: { value: '新渠道' }
  });
  fireEvent.change(screen.getByRole('textbox', { name: '服务地址' }), {
    target: { value: 'https://example.com/custom' }
  });
  fireEvent.click(screen.getByRole('button', { name: '保存并扫码授权' }));
  await act(async () => {});
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
  expect(screen.getByText('请扫码并在设备上确认授权。')).toBeInTheDocument();
  expect(
    document.querySelector('.notification-instance-drawer canvas')
  ).toBeInTheDocument();
  expect(commands[0]?.path).toBe('/api/console/notifications/instances');
  expect(commands[0]?.body).toMatchObject({
    type_code: type.type_code,
    installation_id: type.installation_id,
    config_json: { endpoint: 'https://example.com/custom' }
  });
  expect(commands[1]).toMatchObject({
    path: '/api/console/notifications/instances/persisted-id/authorization',
    body: { type: 'begin', action: 'scan' }
  });
  expect(saved).toHaveBeenCalledWith({ keepOpen: true });
  expect(screen.getByRole('textbox', { name: '渠道名称' })).toHaveValue(
    '新渠道'
  );
  expect(screen.getByRole('textbox', { name: '服务地址' })).toHaveValue(
    'https://example.com/custom'
  );
  expect(screen.getByText('授权完成不代表消息已送达。')).toBeInTheDocument();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(4999);
  });
  expect(
    commands.filter((command) => command.body.type === 'poll')
  ).toHaveLength(0);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });
  expect(screen.getByText('授权状态: 已授权')).toBeInTheDocument();
  expect(
    screen.getByText('授权已完成，渠道启用后可发送消息。')
  ).toBeInTheDocument();
  expect(screen.getByText('通知实例已停用')).toBeInTheDocument();
  expect(commands.some((command) => command.body.status === 'enabled')).toBe(
    false
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(15000);
  });
  expect(
    commands.filter((command) => command.body.type === 'poll')
  ).toHaveLength(1);
});

test('explicit cancel aborts an in-flight poll and ignores its late authorized response', async () => {
  let finish!: (view: NotificationAuthorizationView) => void;
  poll = () =>
    new Promise((resolve) => {
      finish = resolve;
    });
  vi.useFakeTimers();
  render(
    <NotificationInstanceDrawer
      instance={{ ...instance, authorization: pending }}
      type={type}
      onClose={vi.fn()}
      onSaved={saved}
    />
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5000);
  });
  const pollCommand = commands.find((command) => command.body.type === 'poll');
  expect(pollCommand).toBeDefined();
  fireEvent.click(screen.getByRole('button', { name: '取消授权' }));
  await act(async () => {});
  expect(pollCommand?.signal?.aborted).toBe(true);
  expect(screen.getByText('授权状态: 未授权')).toBeInTheDocument();
  await act(async () => {
    finish({ status: 'authorized' });
  });
  expect(screen.queryByText('授权状态: 已授权')).not.toBeInTheDocument();
});

test('unmount stops polling and aborts the request without cancelling backend authorization', async () => {
  poll = () => new Promise(() => {});
  vi.useFakeTimers();
  const rendered = render(
    <NotificationInstanceDrawer
      instance={{ ...instance, authorization: pending }}
      type={type}
      onClose={vi.fn()}
      onSaved={saved}
    />
  );
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5000);
  });
  const pollCommand = commands.find((command) => command.body.type === 'poll');
  rendered.unmount();
  expect(pollCommand?.signal?.aborted).toBe(true);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(15000);
  });
  expect(
    commands.filter((command) => command.body.type === 'poll')
  ).toHaveLength(1);
  expect(commands.some((command) => command.body.type === 'cancel')).toBe(
    false
  );
});

test('invalid authorization offers the declared reauthorization action independently of disabled status', async () => {
  render(
    <NotificationInstanceDrawer
      instance={{ ...instance, authorization: { status: 'invalid' } }}
      type={type}
      onClose={vi.fn()}
      onSaved={saved}
    />
  );
  expect(screen.getByText('授权状态: 授权已失效')).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: '重新授权：扫码授权' }));
  await screen.findByText('请扫码并在设备上确认授权。');
  expect(commands[0]?.body).toEqual({ type: 'begin', action: 'scan' });
  expect(commands[0]?.path).toContain('instance-1/authorization');
});

test('legacy null authorization preserves schema editing and normal save-and-close', async () => {
  render(
    <NotificationInstanceDrawer
      instance={{ ...instance, authorization: null }}
      type={{ ...type, authorization: undefined }}
      onClose={vi.fn()}
      onSaved={saved}
    />
  );
  expect(screen.queryByText(/授权状态/)).not.toBeInTheDocument();
  expect(screen.getByRole('textbox', { name: '通知类型' })).toHaveValue(
    type.display_name
  );
  fireEvent.change(screen.getByRole('textbox', { name: '服务地址' }), {
    target: { value: 'https://example.com/edited' }
  });
  fireEvent.click(screen.getByRole('button', { name: '提交' }));
  await waitFor(() => expect(saved).toHaveBeenCalledWith());
  expect(commands[0]?.body).toEqual({
    display_name: '我的渠道',
    config_json: { endpoint: 'https://example.com/edited' }
  });
  expect(
    commands.some((command) => command.path.endsWith('/authorization'))
  ).toBe(false);
});

test('authorization errors remain safe and do not claim a backend status change', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => {
      throw new Error('SECRET_TOKEN raw provider exception');
    })
  );
  render(
    <NotificationInstanceDrawer
      instance={instance}
      type={type}
      onClose={vi.fn()}
      onSaved={saved}
    />
  );
  fireEvent.click(screen.getByRole('button', { name: '扫码授权' }));
  await screen.findByText('授权操作未完成，请重试。');
  expect(screen.getByText('授权状态: 未授权')).toBeInTheDocument();
  expect(
    screen.queryByText(/SECRET_TOKEN|raw provider exception/)
  ).not.toBeInTheDocument();
  expect(saved).not.toHaveBeenCalled();
});
