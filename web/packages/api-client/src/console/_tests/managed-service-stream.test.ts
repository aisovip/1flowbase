import { afterEach, describe, expect, it, vi } from 'vitest';
import { openManagedServiceStream } from '../managed-service-stream';
class Socket {
  static OPEN = 1;
  static instance: Socket;
  readyState = 1;
  bufferedAmount = 0;
  sent: string[] = [];
  onopen?: () => void;
  onmessage?: (event: { data: string }) => void;
  onclose?: () => void;
  onerror?: () => void;
  constructor(public url: URL) {
    Socket.instance = this;
    queueMicrotask(() => this.onopen?.());
  }
  send(text: string) {
    this.sent.push(text);
  }
  close() {
    this.readyState = 3;
    queueMicrotask(() => this.onclose?.());
  }
}
afterEach(() => vi.unstubAllGlobals());
function setup() {
  const fetch = vi
    .fn()
    .mockResolvedValue(
      new Response(JSON.stringify({ data: { ticket: 'a'.repeat(64) } }))
    );
  vi.stubGlobal('fetch', fetch);
  vi.stubGlobal('window', { location: { origin: 'https://console.example' } });
  vi.stubGlobal('WebSocket', Socket);
  return fetch;
}
describe('managed service duplex bridge', () => {
  it('uses one CSRF ticket request; input never waits for output consumption or HTTP', async () => {
    const fetch = setup();
    let consumed!: () => void;
    const eventDone = new Promise<void>((resolve) => {
      consumed = resolve;
    });
    const onEvent = vi.fn(() => eventDone);
    const stream = await openManagedServiceStream(
      {
        path: '/api/console/managed-services/fixture/terminal',
        body: { host_id: 'host' },
        onEvent
      },
      'csrf'
    );
    const socket = Socket.instance;
    expect(socket.url.href).toBe(
      `wss://console.example/api/console/managed-services/fixture/terminal?ticket=${'a'.repeat(64)}`
    );
    socket.onmessage?.({
      data: JSON.stringify({ type: 'event', id: 0, event: { data: 'output' } })
    });
    await Promise.resolve();
    await stream.send({ data: 'first' });
    await stream.send({ data: 'second' });
    expect(socket.sent.map((text) => JSON.parse(text))).toEqual([
      { type: 'input', input: { data: 'first' } },
      { type: 'input', input: { data: 'second' } }
    ]);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(fetch.mock.calls[0][1].headers['x-csrf-token']).toBe('csrf');
    consumed();
    await vi.waitFor(() =>
      expect(socket.sent.map((text) => JSON.parse(text))).toContainEqual({
        type: 'ack',
        id: 0
      })
    );
    stream.close();
  });
  it('preserves input FIFO while local buffering recovers', async () => {
    setup();
    const stream = await openManagedServiceStream(
      {
        path: '/api/console/managed-services/fixture/terminal',
        onEvent: () => {}
      },
      'csrf'
    );
    const socket = Socket.instance;
    socket.bufferedAmount = 2 * 1024 * 1024;
    const first = stream.send({ data: 'first' });
    await Promise.resolve();
    socket.bufferedAmount = 0;
    const second = stream.send({ data: 'second' });
    const third = stream.send({ data: 'third' });
    await Promise.all([first, second, third]);
    expect(socket.sent.map((text) => JSON.parse(text).input.data)).toEqual([
      'first',
      'second',
      'third'
    ]);
    stream.close();
  });
  it('waits for queued event consumption and complete output when the server closes immediately', async () => {
    setup();
    let consumed!: () => void;
    const done = new Promise<void>((resolve) => {
      consumed = resolve;
    });
    const onClose = vi.fn();
    await openManagedServiceStream(
      {
        path: '/api/console/managed-services/fixture/terminal',
        onEvent: () => done,
        onClose
      },
      'csrf'
    );
    const socket = Socket.instance;
    socket.onmessage?.({
      data: JSON.stringify({ type: 'event', id: 0, event: { data: 'last' } })
    });
    socket.onmessage?.({
      data: JSON.stringify({
        type: 'complete',
        output: { closed: true, reason: 'remote_closed' }
      })
    });
    socket.onclose?.();
    expect(onClose).not.toHaveBeenCalled();
    consumed();
    await vi.waitFor(() =>
      expect(onClose).toHaveBeenCalledWith({
        closed: true,
        reason: 'remote_closed'
      })
    );
  });
  it('rejects unsafe paths and missing CSRF before opening a socket', async () => {
    const fetch = setup();
    await expect(
      openManagedServiceStream(
        { path: 'https://attacker/terminal', onEvent: () => {} },
        'csrf'
      )
    ).rejects.toThrow('Invalid managed service route');
    await expect(
      openManagedServiceStream(
        {
          path: '/api/console/managed-services/fixture/terminal',
          onEvent: () => {}
        },
        null
      )
    ).rejects.toThrow('CSRF');
    expect(fetch).not.toHaveBeenCalled();
  });
  it('passes the final output to onClose after subscriber consumption', async () => {
    setup();
    const onClose = vi.fn();
    await openManagedServiceStream(
      {
        path: '/api/console/managed-services/fixture/terminal',
        onEvent: () => {},
        onClose
      },
      'csrf'
    );
    Socket.instance.onmessage?.({
      data: JSON.stringify({
        type: 'complete',
        output: { closed: true, reason: 'idle_timeout' }
      })
    });
    await vi.waitFor(() =>
      expect(onClose).toHaveBeenCalledWith({
        closed: true,
        reason: 'idle_timeout'
      })
    );
  });
  it('fails closed on malformed events and never acknowledges a failed consumer', async () => {
    setup();
    const onError = vi.fn();
    await openManagedServiceStream(
      {
        path: '/api/console/managed-services/fixture/terminal',
        onEvent: () => {
          throw new Error('consumer failed');
        },
        onError
      },
      'csrf'
    );
    Socket.instance.onmessage?.({
      data: JSON.stringify({ type: 'event', id: 1, event: {} })
    });
    await vi.waitFor(() => expect(onError).toHaveBeenCalled());
    expect(Socket.instance.sent).toEqual([]);
    expect(Socket.instance.readyState).toBe(3);
  });
});
