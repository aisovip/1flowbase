import { ApiClientError } from '../errors';
import { getDefaultApiBaseUrl, unwrapApiSuccess } from '../transport';

export interface ApiUploadProgress {
  loaded: number;
  total?: number;
  complete?: boolean;
}

export interface ApiUploadOptions {
  path: string;
  rawBody: FormData;
  csrfToken: string;
  baseUrl?: string;
  onUploadProgress(progress: ApiUploadProgress): void;
}

/** XHR exposes browser upload bytes; completion still requires the API response. */
export function apiUpload<T>({
  path,
  rawBody,
  csrfToken,
  baseUrl = getDefaultApiBaseUrl(),
  onUploadProgress
}: ApiUploadOptions): Promise<T> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    let progress: ApiUploadProgress = { loaded: 0 };
    xhr.upload.onprogress = (event) => {
      progress = {
        loaded: event.loaded,
        ...(event.lengthComputable ? { total: event.total } : {})
      };
      onUploadProgress(progress);
    };
    xhr.upload.onload = () => onUploadProgress({ ...progress, complete: true });
    xhr.onerror = () => reject(new TypeError('Network request failed'));
    xhr.onabort = () =>
      reject(new DOMException('Upload aborted', 'AbortError'));
    xhr.onload = async () => {
      try {
        if (xhr.status < 200 || xhr.status >= 300) {
          reject(
            await ApiClientError.fromResponse(
              new Response(xhr.responseText, {
                status: xhr.status,
                headers: {
                  'content-type': xhr.getResponseHeader('content-type') ?? ''
                }
              })
            )
          );
          return;
        }
        resolve(
          xhr.status === 204
            ? (undefined as T)
            : unwrapApiSuccess<T>(JSON.parse(xhr.responseText))
        );
      } catch (error) {
        reject(error);
      }
    };
    xhr.open('POST', `${baseUrl}${path}`);
    xhr.withCredentials = true;
    xhr.setRequestHeader('x-csrf-token', csrfToken);
    xhr.send(rawBody);
  });
}
