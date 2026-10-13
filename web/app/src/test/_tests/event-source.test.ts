import { expect, test, vi } from 'vitest';
import { TestEventSource } from '../event-source';

test('transport fixture models explicit open, refresh, reconnect and close without implicit success', () => {
  const stream = new TestEventSource('/events', { withCredentials: true });
  const open = vi.fn();
  const error = vi.fn();
  const refresh = vi.fn();
  stream.onopen = open;
  stream.onerror = error;
  stream.addEventListener('notification.changed', refresh);
  expect(stream.url).toBe('/events');
  expect(stream.withCredentials).toBe(true);
  expect(stream.readyState).toBe(stream.CONNECTING);
  expect(open).not.toHaveBeenCalled();
  stream.dispatchEvent(new Event('open'));
  expect(open).toHaveBeenCalledOnce();
  expect(stream.readyState).toBe(stream.OPEN);
  stream.dispatchEvent(new Event('notification.changed'));
  expect(refresh).toHaveBeenCalledOnce();
  stream.dispatchEvent(new Event('error'));
  expect(error).toHaveBeenCalledOnce();
  expect(stream.readyState).toBe(stream.CONNECTING);
  stream.close();
  stream.dispatchEvent(new Event('notification.changed'));
  expect(refresh).toHaveBeenCalledOnce();
  expect(stream.readyState).toBe(stream.CLOSED);
});
