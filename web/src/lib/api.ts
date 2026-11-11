/*
  Every request to the car goes through here.

  - Writes carry the X-Mibpkg header; the car refuses writes without it, which stops other
    websites from sending requests to the car through this phone.
  - A 401 means a PIN is set and this browser has no session: go to the PIN pad, then come back.
  - Errors become ApiError with the car's plain-words message.
*/

export class ApiError extends Error {
  status: number;
  data: unknown;

  constructor(message: string, status: number, data: unknown = null) {
    super(message);
    this.status = status;
    this.data = data;
  }
}

type Method = 'GET' | 'POST' | 'PUT' | 'DELETE';

const AFTER_LOGIN = 'mibpkg-after-login';

/** Remember where we were, then show the PIN pad. */
function goToLogin() {
  if (!location.hash.startsWith('#/login')) {
    sessionStorage.setItem(AFTER_LOGIN, location.hash || '#/discover');
    location.hash = '#/login';
  }
}

/** Where to go after a correct PIN. */
export function afterLoginHash(): string {
  return sessionStorage.getItem(AFTER_LOGIN) || '#/discover';
}

export function rememberAfterLogin(hash: string) {
  sessionStorage.setItem(AFTER_LOGIN, hash);
}

export async function api<T = unknown>(
  method: Method,
  path: string,
  body: unknown = null,
  { text = false }: { text?: boolean } = {},
): Promise<T> {
  const headers: Record<string, string> = {};
  let payload: BodyInit | null = null;

  if (method !== 'GET') {
    headers['X-Mibpkg'] = '1';
  }
  if (body instanceof Blob) {
    headers['Content-Type'] = 'application/octet-stream';
    payload = body;
  } else if (typeof body === 'string') {
    headers['Content-Type'] = 'text/plain; charset=utf-8';
    payload = body;
  } else if (body !== null) {
    headers['Content-Type'] = 'application/json';
    payload = JSON.stringify(body);
  }

  let response: Response;
  try {
    response = await fetch(path, { method, headers, body: payload, credentials: 'same-origin' });
  } catch {
    throw new ApiError('The car is not answering. Is the phone still on its Wi-Fi?', 0, null);
  }

  if (response.status === 401 && !path.startsWith('/api/login')) {
    goToLogin();
    throw new ApiError('PIN required', 401, null);
  }

  if (text && response.ok) {
    return (await response.text()) as T;
  }

  let data: unknown = null;
  try {
    data = await response.json();
  } catch {
    data = null;
  }

  if (!response.ok) {
    const detail = (data as { error?: string } | null)?.error;
    throw new ApiError(detail || `The car answered ${response.status}`, response.status, data);
  }
  return data as T;
}

/** Upload a file with progress (fetch cannot report upload progress, XMLHttpRequest can). */
export function uploadFile<T>(file: File, onProgress: (fraction: number) => void): Promise<T> {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('POST', '/api/uploads');
    xhr.setRequestHeader('X-Mibpkg', '1');
    xhr.setRequestHeader('Content-Type', 'application/octet-stream');

    xhr.upload.onprogress = (event) => {
      if (event.lengthComputable) {
        onProgress(event.loaded / event.total);
      }
    };
    xhr.onload = () => {
      let data: unknown = null;
      try {
        data = JSON.parse(xhr.responseText);
      } catch {
        data = null;
      }
      if (xhr.status === 401) {
        goToLogin();
        reject(new ApiError('PIN required', 401, null));
      } else if (xhr.status >= 200 && xhr.status < 300) {
        resolve(data as T);
      } else {
        const detail = (data as { error?: string } | null)?.error;
        reject(new ApiError(detail || `The car answered ${xhr.status}`, xhr.status, data));
      }
    };
    xhr.onerror = () => reject(new ApiError("The upload was interrupted. Is the phone still on the car's Wi-Fi?", 0, null));
    xhr.send(file);
  });
}
