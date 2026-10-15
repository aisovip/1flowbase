import { afterEach, expect, test, vi } from 'vitest';
import {
  createNotificationInstance,
  authorizeNotificationInstance,
  type NotificationAuthorizationView,
  listNotificationMessages,
  markNotificationRead,
  subscribeNotificationChanges,
  updateNotificationInstance
} from '../index';
afterEach(() => vi.unstubAllGlobals());
test('uses canonical fields, envelope, credential and csrf transport', async () => {
  const fetch = vi
    .fn()
    .mockResolvedValue(
      new Response(JSON.stringify({ data: { id: 'created' }, meta: null }))
    );
  vi.stubGlobal('fetch', fetch);
  const result = await createNotificationInstance(
    {
      type_code: 'system',
      installation_id: null,
      display_name: 'Test',
      config_json: {}
    },
    'csrf'
  );
  expect(result).toEqual({ id: 'created' });
  expect(fetch).toHaveBeenCalledWith(
    expect.stringContaining('/api/console/notifications/instances'),
    expect.objectContaining({
      method: 'POST',
      credentials: 'include',
      headers: { 'content-type': 'application/json', 'x-csrf-token': 'csrf' },
      body: JSON.stringify({
        type_code: 'system',
        installation_id: null,
        display_name: 'Test',
        config_json: {}
      })
    })
  );
  fetch.mockImplementation(
    async () => new Response(JSON.stringify({ data: {}, meta: null }))
  );
  await updateNotificationInstance('id/1', { status: 'disabled' }, 'csrf');
  expect(fetch).toHaveBeenLastCalledWith(
    expect.stringContaining('/instances/id%2F1'),
    expect.objectContaining({ method: 'PATCH', body: '{"status":"disabled"}' })
  );
  await listNotificationMessages({
    cursor: 'next cursor',
    page_size: 10,
    unread_only: true
  });
  expect(fetch).toHaveBeenLastCalledWith(
    expect.stringContaining('cursor=next+cursor&page_size=10&unread_only=true'),
    expect.anything()
  );
  await markNotificationRead('id/2', 'csrf');
  expect(fetch).toHaveBeenLastCalledWith(
    expect.stringContaining('/messages/id%2F2/read'),
    expect.objectContaining({ method: 'POST' })
  );
});
test('refresh signals and every open reread persistent state; close releases source', () => {
  const refresh = vi.fn();
  const connection = vi.fn();
  const sources: TestEventSource[] = [];
  class TestEventSource {
    onopen?: () => void;
    onerror?: () => void;
    addEventListener = vi.fn();
    close = vi.fn();
    constructor(url: string, options: unknown) {
      expect(url).toBe('https://example.com/api/console/notifications/events');
      expect(options).toEqual({ withCredentials: true });
      sources.push(this);
    }
  }
  vi.stubGlobal('EventSource', TestEventSource);
  const close = subscribeNotificationChanges(
    refresh,
    connection,
    'https://example.com'
  );
  const source = sources[0]!;
  source.onopen?.();
  source.onerror?.();
  source.onopen?.();
  expect(refresh).toHaveBeenCalledTimes(2);
  expect(source.addEventListener).toHaveBeenCalledWith(
    'notification.changed',
    refresh
  );
  close();
  expect(source.close).toHaveBeenCalledOnce();
});

test('authorization posts canonical commands and reads only the public view', async () => {
  const { authorizeNotificationInstance } = await import('../index');
  const authorization = {
    status: 'pending',
    user_action: {
      kind: 'qr_code',
      qr_code_url: 'https://example.com/qr',
      poll_interval_seconds: 5,
      expires_at: '2026-10-15T02:00:00Z'
    }
  };
  const fetch = vi.fn(
    async () =>
      new Response(JSON.stringify({ data: authorization, meta: null }))
  );
  vi.stubGlobal('fetch', fetch);
  const controller = new AbortController();
  for (const body of [
    { type: 'begin', action: 'scan' } as const,
    { type: 'poll' } as const,
    { type: 'cancel' } as const,
    { type: 'maintain' } as const
  ]) {
    expect(
      await authorizeNotificationInstance(
        'id/1',
        body,
        'csrf',
        controller.signal
      )
    ).toEqual(authorization);
    expect(fetch).toHaveBeenLastCalledWith(
      expect.stringContaining(
        '/api/console/notifications/instances/id%2F1/authorization'
      ),
      expect.objectContaining({
        method: 'POST',
        credentials: 'include',
        headers: { 'content-type': 'application/json', 'x-csrf-token': 'csrf' },
        body: JSON.stringify(body),
        signal: controller.signal
      })
    );
  }
});

test('Maintain preserves backend delivery window field names and timestamps', async () => {
  const authorization: NotificationAuthorizationView = {
    status: 'authorized',
    delivery_window: {
      status: 'active',
      last_received_at_ms: 1_760_000_000_000,
      expires_at_ms: 1_760_086_400_000
    }
  };
  const fetch = vi.fn(
    async () =>
      new Response(JSON.stringify({ data: authorization, meta: null }))
  );
  vi.stubGlobal('fetch', fetch);
  expect(
    await authorizeNotificationInstance(
      'instance-1',
      { type: 'maintain' },
      'csrf'
    )
  ).toEqual(authorization);
  expect(fetch).toHaveBeenCalledWith(
    expect.stringContaining('/instances/instance-1/authorization'),
    expect.objectContaining({ method: 'POST', body: '{"type":"maintain"}' })
  );
});
