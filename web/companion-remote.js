'use strict';
// Loaded only by the opt-in phone listener, before the desktop transport.
window.DMRemote = true;
(() => {
  const send = window.fetch.bind(window);
  let csrf = '';
  let session;
  const invitation = new URLSearchParams(location.hash.slice(1)).get('invite') || '';
  if (invitation) history.replaceState(null, '', location.pathname + location.search);
  async function credentials() {
    if (csrf) return csrf;
    session ??= send('/api/session', {cache: 'no-store'}).then(async r => {
      if (!r.ok) throw new Error('Pair this phone again from Connections on desktop.');
      csrf = (await r.json()).csrf;
      return csrf;
    }).finally(() => { session = null; });
    return session;
  }
  const failure = message => new Response(JSON.stringify({error: message, code: 'phone_access'}),
    {status: 403, headers: {'Content-Type': 'application/json'}});
  window.fetch = async (input, init) => {
    const request = new Request(input, init);
    const url = new URL(request.url);
    if (url.origin !== location.origin || !url.pathname.startsWith('/api/')) return send(request);
    if (request.method !== 'POST') return send(request);
    try { request.headers.set('X-DM-CSRF', await credentials()); }
    catch (error) { return failure(error.message); }
    let payload;
    if (['/api/upload', '/api/itch-upload'].includes(url.pathname)) {
      const blob = await request.blob();
      if (!blob.size || blob.size > 50 * 1024 * 1024) return failure('Choose a nonempty file up to 50 MiB.');
      const base64 = await new Promise((resolve, reject) => {
        const reader = new FileReader(); reader.onload = () => resolve(String(reader.result).split(',', 2)[1]);
        reader.onerror = () => reject(new Error('The file could not be read.')); reader.readAsDataURL(blob);
      });
      const field = name => { try { return decodeURIComponent(request.headers.get(name) || ''); } catch { return ''; } };
      payload = {base64, filename: field('X-File-Name'), mime_type: field('X-File-Type') || blob.type,
        thread_id: field('X-Thread-Id'), video_mode: field('X-Video-Mode'), t3_target: field('X-T3-Target')};
    } else if (url.pathname === '/api/call') {
      payload = await request.clone().json();
      if (payload.name === 'manager.social.gif' && window.DMNativeCards) {
        const call = async (name, args) => {
          const response = await send('/api/call', {method: 'POST', signal: request.signal,
            headers: {'Content-Type': 'application/json', 'X-DM-CSRF': csrf}, body: JSON.stringify({name, args})});
          const value = await response.json();
          if (!response.ok) throw new Error(value.error || 'Card rendering failed.');
          return value;
        };
        try {
          const value = await window.DMNativeCards.gif(payload.args || {}, call, {signal: request.signal});
          return new Response(JSON.stringify(value), {headers: {'Content-Type': 'application/json'}});
        } catch (error) {
          return new Response(JSON.stringify({error: error.message || 'Card rendering failed.'}),
            {status: 503, headers: {'Content-Type': 'application/json'}});
        }
      }
    }
    if (payload && ['/api/upload', '/api/itch-upload'].includes(url.pathname)) {
      return send(url, {method: 'POST', signal: request.signal,
        headers: {'Content-Type': 'application/json', 'X-DM-CSRF': csrf}, body: JSON.stringify(payload)});
    }
    return send(request);
  };
  document.addEventListener('DOMContentLoaded', () => {
    const button = document.querySelector('#phone-pair-button');
    if (!button) return;
    const status = document.querySelector('#phone-pair-status');
    if (!invitation) return;
    status.textContent = 'Invitation ready. Connect only if you recognize the desktop that supplied it.';
    button.hidden = false;
    button.onclick = async () => {
      button.disabled = true;
      try {
        const r = await send('/api/pair', {method: 'POST', headers: {'Content-Type': 'application/json'},
          body: JSON.stringify({invite: invitation})});
        const result = await r.json();
        if (!r.ok) throw new Error(result.error || 'Pairing failed.');
        location.reload();
      } catch (error) { status.textContent = error.message; button.disabled = false; }
    };
  }, {once: true});
})();
