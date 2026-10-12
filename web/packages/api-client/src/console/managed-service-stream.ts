import { requestManagedService } from './managed-services';

export interface ManagedServiceStreamOptions {
  path: string;
  body?: unknown;
  onEvent: (event: unknown) => void | Promise<void>;
  onClose?: (output?: unknown) => void;
  onError?: (error: Error) => void;
  signal?: AbortSignal;
}
export interface ManagedServiceStream {
  send(input: unknown): Promise<void>;
  close(): void;
}
const FRAME_BYTES = 1024 * 1024;
const HIGH_WATER_BYTES = FRAME_BYTES;

/** One CSRF-protected admission; all subsequent traffic stays on the same-origin socket. */
export async function openManagedServiceStream(
  options: ManagedServiceStreamOptions,
  csrfToken: string | null
): Promise<ManagedServiceStream> {
  if (!csrfToken)
    throw new Error('A cookie session with CSRF protection is required');
  if (options.signal?.aborted) throw new DOMException('Aborted', 'AbortError');
  const ticket = await requestManagedService<{ ticket: string }>(
    {
      path: options.path,
      method: 'POST',
      body: options.body,
      signal: options.signal
    },
    csrfToken
  );
  if (options.signal?.aborted) throw new DOMException('Aborted', 'AbortError');
  if (!/^[a-f0-9]{64}$/.test(ticket.ticket))
    throw new Error('Invalid stream ticket');
  const url = new URL(options.path, window.location.origin);
  if (url.origin !== window.location.origin)
    throw new Error('Invalid managed service origin');
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  url.searchParams.set('ticket', ticket.ticket);
  const socket = new WebSocket(url);
  let closed = false;
  let stopped = false;
  let failed = false;
  let output: unknown;
  const close = () => {
    stopped = true;
    if (!closed) {
      closed = true;
      socket.close();
    }
  };
  const fail = (error: unknown) => {
    if (failed) return;
    failed = true;
    close();
    options.onError?.(
      error instanceof Error ? error : new Error('Stream transport failed')
    );
  };
  const encode = (value: unknown) => {
    const text = JSON.stringify(value);
    if (new TextEncoder().encode(text).byteLength > FRAME_BYTES)
      throw new Error('Stream frame too large');
    return text;
  };
  let sending = Promise.resolve();
  const send = (input: unknown): Promise<void> => {
    let frame: string;
    try {
      frame = encode({ type: 'input', input });
    } catch (error) {
      return Promise.reject(error);
    }
    const admitted = sending.then(async () => {
      while (socket.bufferedAmount > HIGH_WATER_BYTES && !closed) {
        await new Promise((resolve) => setTimeout(resolve, 4));
      }
      if (closed || socket.readyState !== WebSocket.OPEN)
        throw new Error('Stream is closed');
      socket.send(frame);
    });
    sending = admitted.catch(() => {});
    return admitted;
  };
  const abort = () => close();
  options.signal?.addEventListener('abort', abort, { once: true });
  let consuming = Promise.resolve();
  socket.onmessage = (message) => {
    // Ordered callback resolution is the subscriber acknowledgement, separate from input traffic.
    consuming = consuming
      .then(async () => {
        if (stopped) return;
        if (
          typeof message.data !== 'string' ||
          new TextEncoder().encode(message.data).byteLength > FRAME_BYTES
        ) {
          throw new Error('Invalid stream frame');
        }
        const frame = JSON.parse(message.data);
        if (
          frame.type === 'event' &&
          Number.isSafeInteger(frame.id) &&
          frame.id >= 0
        ) {
          await options.onEvent(frame.event);
          if (!closed && socket.readyState === WebSocket.OPEN)
            socket.send(encode({ type: 'ack', id: frame.id }));
        } else if (frame.type === 'complete') {
          output = frame.output;
          close();
        } else if (
          frame.type === 'error' &&
          typeof frame.message === 'string'
        ) {
          throw new Error(frame.message);
        } else {
          throw new Error('Invalid stream frame');
        }
      })
      .catch(fail);
  };
  socket.onclose = () => {
    closed = true;
    options.signal?.removeEventListener('abort', abort);
    void consuming.then(() => options.onClose?.(output));
  };
  await new Promise<void>((resolve, reject) => {
    socket.onopen = () => {
      if (options.signal?.aborted || closed) {
        close();
        reject(new DOMException('Aborted', 'AbortError'));
      } else resolve();
    };
    socket.onerror = () => {
      const error = new Error('Stream connection failed');
      reject(error);
      void consuming.then(() => {
        if (!stopped) fail(error);
      });
    };
    const previousClose = socket.onclose;
    socket.onclose = (event) => {
      previousClose?.call(socket, event);
      reject(new Error('Stream is closed'));
    };
  });
  return { send, close };
}
