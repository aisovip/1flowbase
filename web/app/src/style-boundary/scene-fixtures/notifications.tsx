import { useLayoutEffect, useRef } from 'react';
import type { NotificationMessage } from '@1flowbase/api-client';
import { NotificationBell } from '../../features/notifications/components/NotificationBell';

const messages: NotificationMessage[] = [
  {
    id: 'notification-unread',
    workspace_id: 'workspace-1',
    instance_id: 'notification-instance',
    recipient_user_id: 'user-1',
    title: '工作流运行完成，请检查处理结果',
    content: `第一行：团队工作流已完成。\n第二行：查看结果 https://example.com/${'notification-result-'.repeat(12)}`,
    read_at: null,
    created_at: '2026-10-13T00:00:00Z'
  },
  {
    id: 'notification-read',
    workspace_id: 'workspace-1',
    instance_id: 'notification-instance',
    recipient_user_id: 'user-1',
    title: '已处理的通知',
    content: '已读消息保留在当前收件箱中。',
    read_at: '2026-10-13T00:01:00Z',
    created_at: '2026-10-13T00:00:00Z'
  }
];

export function seedStyleBoundaryNotificationFetch() {
  const originalFetch = globalThis.fetch.bind(globalThis);
  globalThis.fetch = async (input, init) => {
    const url = new URL(
      typeof input === 'string'
        ? input
        : input instanceof URL
          ? input.href
          : input.url,
      window.location.origin
    );
    const payload =
      url.pathname === '/api/console/notifications/messages/unread-count'
        ? { unread_count: 1 }
        : url.pathname === '/api/console/notifications/messages'
          ? { items: messages, next_cursor: null, unread_count: 1 }
          : undefined;
    if (payload !== undefined) {
      return new Response(JSON.stringify({ data: payload, meta: null }), {
        status: 200,
        headers: { 'content-type': 'application/json' }
      });
    }
    return originalFetch(input, init);
  };
}

// The style scene supplies a stable connected signal, while the actual inbox
// consumes its HTTP snapshot and renders the production bell and drawer.
class NotificationSceneEventSource extends EventTarget {
  onopen: ((event: Event) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  private closed = false;
  constructor() {
    super();
    queueMicrotask(() => {
      if (!this.closed) this.onopen?.(new Event('open'));
    });
  }
  close() {
    this.closed = true;
  }
}

export function NotificationInboxStyleBoundaryScene() {
  const root = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const originalEventSource = globalThis.EventSource;
    globalThis.EventSource =
      NotificationSceneEventSource as unknown as typeof EventSource;
    root.current?.querySelector('button')?.click();
    return () => {
      globalThis.EventSource = originalEventSource;
    };
  }, []);
  return (
    <div ref={root} data-testid="notification-bell-scene">
      <NotificationBell />
    </div>
  );
}
