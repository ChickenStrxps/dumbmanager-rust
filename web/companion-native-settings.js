'use strict';
(() => {
  if (window.DMRemote || !window.DMT3) return;
  const t3 = window.DMT3, markup = t3.markup.bind(t3), sync = t3.sync.bind(t3);
  t3.markup = () => markup() + `<details class="tool-card native-settings-card" id="phone-native-settings"><summary>Phone connection</summary>
    <p>Start a paired connection to this Rust app. Nothing runs until you start it. Direct HTTP is for a trusted local network; remote access requires your own HTTPS proxy.</p>
    <label for="phone-bind">Local IPv4 address</label><input id="phone-bind" autocomplete="off" placeholder="127.0.0.1">
    <label for="phone-port">Port</label><input id="phone-port" type="number" min="1" max="65535" value="8877">
    <label for="phone-proxy">HTTPS proxy origin (optional; requires loopback address)</label><input id="phone-proxy" type="url" autocomplete="off" placeholder="https://your-host.example">
    <div class="detail-actions"><button class="secondary" type="button" data-phone="configure">Save address</button><button class="primary" type="button" data-phone="start">Start</button><button class="secondary" type="button" data-phone="stop">Stop</button><button class="secondary" type="button" data-phone="invite">Create phone invitation</button><button class="quiet" type="button" data-phone="revoke">Revoke all devices</button></div>
    <p id="phone-native-status" role="status" aria-live="polite"></p><p id="phone-native-invite"></p></details>`;
  t3.sync = () => {
    sync(); const root = document.querySelector('#phone-native-settings'); if (!root) return;
    const status = root.querySelector('#phone-native-status'), invite = root.querySelector('#phone-native-invite');
    const update = value => {
      root.querySelector('#phone-bind').value = value.bind;
      root.querySelector('#phone-port').value = value.port;
      root.querySelector('#phone-proxy').value = value.proxy_origin;
      status.textContent = value.running ? 'Phone listener running. Paired devices have access to itch and chat flows. Native settings, Git and system controls stay on desktop.' : 'Phone listener stopped.';
    };
    let loaded = false;
    root.addEventListener('toggle', async () => {
      if (!root.open || loaded) return;
      try { update(await call('companion.status')); loaded = true; } catch (error) { status.textContent = error.message; }
    });
    root.onclick = async event => {
      const button = event.target.closest('[data-phone]'); if (!button || button.disabled) return;
      const buttons = [...root.querySelectorAll('button')]; buttons.forEach(b => { b.disabled = true; });
      try {
        const action = button.dataset.phone;
        if (action === 'configure') update(await call('companion.configure', {bind: root.querySelector('#phone-bind').value,
          port: Number(root.querySelector('#phone-port').value), proxy_origin: root.querySelector('#phone-proxy').value}));
        else if (action === 'invite') {
          const value = await call('companion.invite'); invite.replaceChildren();
          const link = document.createElement('a'); link.href = value.url; link.textContent = 'Open one-time phone invitation';
          link.target = '_blank'; link.rel = 'noopener noreferrer'; invite.append(link);
          status.textContent = 'Invitation expires in five minutes and works once. Share it only with your own device.';
          const copy = document.createElement('button'); copy.className = 'quiet'; copy.type = 'button'; copy.textContent = 'Copy phone link';
          copy.onclick = async () => { try { await navigator.clipboard.writeText(value.url); status.textContent = 'Phone invitation copied.'; } catch { status.textContent = 'Copy the invitation link manually.'; } };
          invite.append(copy);
        } else { update(await call(`companion.${action}`)); invite.replaceChildren(); }
      } catch (error) { status.textContent = error.message; }
      finally { buttons.forEach(b => { b.disabled = false; }); }
    };
  };
})();
