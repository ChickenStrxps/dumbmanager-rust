'use strict';

// Keep the original UI's HTTP-shaped API local to this app. Asset fetches keep
// their browser implementation; no legacy API request falls through to a server.
(() => {
  if (window.DMRemote === true) return;
  const assetFetch = window.fetch.bind(window);
  const json = (value, status = 200) => new Response(JSON.stringify(value ?? null), {
    status,
    headers: {'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'no-store'}
  });
  const unavailable = () => json({
    error: 'Local data requires the DumbManager Rust desktop app.',
    code: 'native_unavailable'
  }, 503);

  const UPLOAD_LIMIT = 50 * 1024 * 1024;
  function header(request, name) {
    const value = request.headers.get(name) || '';
    try { return decodeURIComponent(value); } catch { return value; }
  }
  async function upload(request) {
    const blob = await request.blob();
    if (!blob.size || blob.size > UPLOAD_LIMIT) {
      throw {error: 'Choose a nonempty file no larger than 50 MiB.', code: 'invalid_upload', status: 413};
    }
    const base64 = await new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result).split(',', 2)[1]);
      reader.onerror = () => reject({error: 'The file could not be read.', code: 'invalid_upload', status: 400});
      reader.readAsDataURL(blob);
    });
    return {base64, filename: header(request, 'X-File-Name'),
      mime_type: header(request, 'X-File-Type') || blob.type || 'application/octet-stream',
      thread_id: header(request, 'X-Thread-Id'), video_mode: header(request, 'X-Video-Mode'),
      t3_target: header(request, 'X-T3-Target')};
  }
  function response(value) {
    if (value?.__native_binary === true) {
      const raw = atob(value.base64);
      const bytes = new Uint8Array(raw.length);
      for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
      return new Response(bytes, {status: value.status || 200,
        headers: {'Content-Type': value.mime_type || 'application/octet-stream', 'Cache-Control': 'no-store'}});
    }
    return json(value);
  }

  function failure(error) {
    let detail = error;
    if (typeof detail === 'string') {
      try { detail = JSON.parse(detail); } catch { /* A readable Tauri rejection. */ }
    }
    const object = detail && typeof detail === 'object' ? detail : {};
    const message = typeof object.error === 'string' ? object.error :
      typeof object.message === 'string' ? object.message :
      typeof detail === 'string' ? detail : 'The native request failed.';
    const candidate = Number(object.status ?? object.status_code);
    const status = Number.isInteger(candidate) && candidate >= 400 && candidate <= 599 ? candidate : 503;
    const body = {error: message, code: typeof object.code === 'string' ? object.code : 'native_error'};
    if (Number.isFinite(object.retry_after_ms)) body.retry_after_ms = object.retry_after_ms;
    return json(body, status);
  }

  window.fetch = async function nativeFetch(input, init) {
    let url;
    try { url = new URL(input instanceof Request ? input.url : String(input), location.href); }
    catch { return assetFetch(input, init); }
    const method = String(init?.method ?? (input instanceof Request ? input.method : 'GET')).toUpperCase();
    if (url.origin !== location.origin || !url.pathname.startsWith('/api/') || method !== 'POST') {
      return assetFetch(input, init);
    }

    const request = new Request(input, init);
    request.signal.throwIfAborted();
    const type = request.headers.get('Content-Type') || '';
    let payload;
    try {
      if (url.pathname === '/api/upload' || url.pathname === '/api/itch-upload') payload = await upload(request);
      else {
        if (request.body && !/^application\/(?:[\w.+-]+\+)?json(?:\s*;|\s*$)/i.test(type)) {
          return json({error: 'This native API requires JSON.', code: 'invalid_payload'}, 415);
        }
        payload = JSON.parse((await request.text()) || '{}');
      }
    } catch (error) {
      return error?.status ? failure(error) : json({error: 'The native API requires a JSON object.', code: 'invalid_payload'}, 400);
    }
    request.signal.throwIfAborted();
    if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
      return json({error: 'The native API requires a JSON object.', code: 'invalid_payload'}, 400);
    }
    const invoke = window.__TAURI__?.core?.invoke;
    if (typeof invoke !== 'function') return unavailable();

    let abort;
    const aborted = new Promise((_, reject) => {
      abort = () => reject(request.signal.reason ?? new DOMException('The operation was aborted.', 'AbortError'));
      request.signal.addEventListener('abort', abort, {once: true});
      if (request.signal.aborted) abort();
    });
    const operation = (async () => {
      try {
        const callNative = (name, args) => invoke('legacy_api', {path: '/api/call', payload: {name, args}});
        const value = url.pathname === '/api/call' && !url.search && payload.name === 'manager.social.gif'
          && window.DMNativeCards
          ? await window.DMNativeCards.gif(payload.args || {}, callNative, {signal: request.signal})
          : await invoke('legacy_api', {path: url.pathname + url.search, payload});
        return response(value);
      }
      catch (error) { return failure(error); }
    })();
    try { return await Promise.race([operation, aborted]); }
    finally { request.signal.removeEventListener('abort', abort); }
  };
})();
