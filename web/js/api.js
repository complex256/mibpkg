/*
  api.js: every request to the car goes through here.

  - Writes carry the X-Mibpkg header; the car refuses writes without it, which
    stops other websites from sending requests to the car through this phone.
  - A 401 means a PIN is set and this browser has no session: go to the PIN
    pad, then come back.
  - Errors become ApiError with the car's plain-words message.
*/

export class ApiError extends Error {
  constructor(message, status, data) {
    super(message);
    this.status = status;
    this.data = data;
  }
}

/**
 * Call the car's API.
 *   method: 'GET', 'POST', 'PUT' or 'DELETE'
 *   path:   e.g. '/api/packages'
 *   body:   an object (sent as JSON), a string, a Blob/File (sent as is), or null
 *   text:   true to return the response as text instead of JSON
 */
export async function api(method, path, body = null, { text = false } = {}) {
  const headers = {};
  let payload = null;

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

  let response;
  try {
    response = await fetch(path, { method, headers, body: payload, credentials: 'same-origin' });
  } catch (e) {
    throw new ApiError('The car is not answering. Is the phone still on its Wi-Fi?', 0, null);
  }

  if (response.status === 401 && !path.startsWith('/api/login')) {
    goToLogin();
    throw new ApiError('PIN required', 401, null);
  }

  if (text && response.ok) {
    return response.text();
  }

  let data = null;
  try {
    data = await response.json();
  } catch (e) {
    data = null;
  }
  if (!response.ok && response.status !== 409) {
    const message = (data && data.error) || `The car answered ${response.status}`;
    throw new ApiError(message, response.status, data);
  }
  if (response.status === 409) {
    // "busy" (or "needs confirmation"): callers look at data themselves.
    throw new ApiError((data && data.error) || 'Busy', 409, data);
  }
  return data;
}

/** Remember where we were, then show the PIN pad. */
function goToLogin() {
  if (!location.hash.startsWith('#/login')) {
    sessionStorage.setItem('mibpkg-after-login', location.hash || '#/discover');
    location.hash = '#/login';
  }
}

/**
 * Upload a file with progress (fetch cannot report upload progress, XMLHttpRequest can).
 * onProgress(fraction 0..1). Resolves with the parsed JSON reply.
 */
export function uploadFile(file, onProgress) {
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('POST', '/api/uploads');
    xhr.setRequestHeader('X-Mibpkg', '1');
    xhr.setRequestHeader('Content-Type', 'application/octet-stream');

    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable) {
        onProgress(e.loaded / e.total);
      }
    };
    xhr.onload = () => {
      let data = null;
      try {
        data = JSON.parse(xhr.responseText);
      } catch (e) {
        data = null;
      }
      if (xhr.status === 401) {
        goToLogin();
        reject(new ApiError('PIN required', 401, null));
      } else if (xhr.status >= 200 && xhr.status < 300) {
        resolve(data);
      } else {
        reject(new ApiError((data && data.error) || `The car answered ${xhr.status}`, xhr.status, data));
      }
    };
    xhr.onerror = () => reject(new ApiError('The upload was interrupted. Is the phone still on the car\'s Wi-Fi?', 0, null));
    xhr.send(file);
  });
}
