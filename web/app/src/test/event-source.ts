/** Deterministic browser transport for DOM tests. Tests deliver events explicitly;
 * construction never opens a network connection or invents a successful session. */
export class TestEventSource extends EventTarget {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSED = 2;
  readonly CONNECTING = 0;
  readonly OPEN = 1;
  readonly CLOSED = 2;
  readonly url: string;
  readonly withCredentials: boolean;
  readyState = TestEventSource.CONNECTING;
  onopen: ((event: Event) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;

  constructor(url: string | URL, options: EventSourceInit = {}) {
    super();
    this.url = String(url);
    this.withCredentials = options.withCredentials ?? false;
  }

  override dispatchEvent(event: Event) {
    if (this.readyState === TestEventSource.CLOSED) return false;
    if (event.type === 'open') {
      this.readyState = TestEventSource.OPEN;
      this.onopen?.(event);
    } else if (event.type === 'error') {
      this.readyState = TestEventSource.CONNECTING;
      this.onerror?.(event);
    } else if (event.type === 'message') {
      this.onmessage?.(event as MessageEvent);
    }
    return super.dispatchEvent(event);
  }

  close() {
    this.readyState = TestEventSource.CLOSED;
  }
}
