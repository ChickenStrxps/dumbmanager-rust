'use strict';

// Keep the original UI's HTTP-shaped API local to this app. Asset fetches keep
// their browser implementation; no legacy API request falls through to a server.
(() => {
  const assetFetch = window.fetch.bind(window);
  const json = (value, status = 200) => new Response(JSON.stringify(value ?? null), {
    status,
    headers: {'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'no-store'}
  });
  const unavailable = () => json({
    error: 'Local data requires the DumbManager Rust desktop app.',
    code: 'native_unavailable'
  }, 503);

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
    if (request.body && !/^application\/(?:[\w.+-]+\+)?json(?:\s*;|\s*$)/i.test(type)) {
      return json({error: 'Native file uploads are unavailable in this build.', code: 'native_upload_unavailable'}, 415);
    }
    let payload;
    try { payload = JSON.parse((await request.text()) || '{}'); }
    catch { return json({error: 'The native API requires a JSON object.', code: 'invalid_payload'}, 400); }
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
      try { return json(await invoke('legacy_api', {path: url.pathname + url.search, payload})); }
      catch (error) { return failure(error); }
    })();
    try { return await Promise.race([operation, aborted]); }
    finally { request.signal.removeEventListener('abort', abort); }
  };
})();
