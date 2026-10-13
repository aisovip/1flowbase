import { afterEach, describe, expect, test, vi } from 'vitest';
import { apiUpload } from '../upload';

class FakeXhr {
  static current: FakeXhr;
  upload = {
    onprogress: null as ((event: ProgressEvent) => void) | null,
    onload: null as (() => void) | null
  };
  onload: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onabort: (() => void) | null = null;
  status = 200;
  responseText = '{"data":{"installed":true}}';
  withCredentials = false;
  open = vi.fn();
  setRequestHeader = vi.fn();
  send = vi.fn();
  getResponseHeader = () => 'application/json';
  constructor() {
    FakeXhr.current = this;
  }
}

afterEach(() => vi.unstubAllGlobals());

describe('authenticated multipart upload', () => {
  function begin() {
    vi.stubGlobal('XMLHttpRequest', FakeXhr);
    const rawBody = new FormData();
    rawBody.set('file', new Blob(['binary']), 'plugin.1flowbasepkg');
    const progress = vi.fn();
    const result = apiUpload({
      path: '/upload',
      baseUrl: 'http://localhost:7800',
      csrfToken: 'csrf',
      rawBody,
      onUploadProgress: progress
    });
    return { xhr: FakeXhr.current, progress, result, rawBody };
  }

  test('real byte progress and upload EOF do not resolve installation before the response', async () => {
    const { xhr, progress, result, rawBody } = begin();
    const resolved = vi.fn();
    result.then(resolved);
    expect(xhr.open).toHaveBeenCalledWith(
      'POST',
      'http://localhost:7800/upload'
    );
    expect(xhr.withCredentials).toBe(true);
    expect(xhr.setRequestHeader.mock.calls).toEqual([['x-csrf-token', 'csrf']]);
    expect(xhr.send).toHaveBeenCalledWith(rawBody);
    xhr.upload.onprogress?.({
      loaded: 50,
      total: 100,
      lengthComputable: true
    } as ProgressEvent);
    expect(progress).toHaveBeenLastCalledWith({ loaded: 50, total: 100 });
    xhr.upload.onload?.();
    expect(progress).toHaveBeenLastCalledWith({
      loaded: 50,
      total: 100,
      complete: true
    });
    await Promise.resolve();
    expect(resolved).not.toHaveBeenCalled();
    xhr.onload?.();
    await expect(result).resolves.toEqual({ installed: true });
  });

  test('unknown totals retain byte progress and 409 keeps the backend risk body', async () => {
    const { xhr, progress, result } = begin();
    xhr.upload.onprogress?.({
      loaded: 25,
      total: 0,
      lengthComputable: false
    } as ProgressEvent);
    expect(progress).toHaveBeenLastCalledWith({ loaded: 25 });
    const body = {
      code: 'extension_risk_confirmation_required',
      message: 'Confirm risk',
      risk_challenge: { warnings: [] }
    };
    xhr.status = 409;
    xhr.responseText = JSON.stringify(body);
    xhr.onload?.();
    await expect(result).rejects.toMatchObject({
      name: 'ApiClientError',
      status: 409,
      code: body.code,
      body
    });
  });

  test('network failure and malformed success are rejected rather than installed', async () => {
    const first = begin();
    first.xhr.onerror?.();
    await expect(first.result).rejects.toBeInstanceOf(TypeError);
    const second = begin();
    second.xhr.responseText = 'not json';
    second.xhr.onload?.();
    await expect(second.result).rejects.toBeInstanceOf(SyntaxError);
  });
});
