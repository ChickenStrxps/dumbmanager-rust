'use strict';
// Add controls to the existing Connections settings extension point.
(() => {
  const settings = window.DMT3;
  if (!settings) return;
  const markup = settings.markup.bind(settings), sync = settings.sync.bind(settings);
  let candidate = null, busy = false, message = 'Check for a signed update.';
  let autoCheck = false;
  try { autoCheck = localStorage.getItem('dm.check-updates') === '1'; } catch (_) { /* Manual checks remain available. */ }
  settings.markup = () => markup() + `<details class="tool-card native-settings-card" id="native-updates"><summary>Application updates</summary>
    <p>Save your work before installing; the app will close. Downloads must pass signature verification.</p>
    <label><input type="checkbox" id="update-auto" ${autoCheck?'checked':''}> Check for updates when the app opens</label>
    <button type="button" class="secondary" id="update-check">Check for updates</button>
    <button type="button" class="secondary" id="update-install" hidden>Install update</button>
    <p id="update-status" role="status" aria-live="polite"></p></details>`;
  function paint() {
    const card = document.getElementById('native-updates');
    if (!card) return;
    card.querySelector('#update-status').textContent = message;
    card.querySelector('#update-check').disabled = busy;
    const install = card.querySelector('#update-install');
    install.hidden = !candidate; install.disabled = busy;
    if (candidate) install.textContent = `Install ${candidate.version}`;
  }
  async function check(automatic = false) {
      if (busy) return;
      busy = true; candidate = null; message = 'Checking for updates…'; paint();
      try {
        candidate = await window.__TAURI__.core.invoke('update_check');
        message = candidate ? `Version ${candidate.version} is available (installed: ${candidate.current_version}).` : 'You have the latest available version.';
        if (automatic && candidate && typeof toast === 'function') toast(`DumbManager ${candidate.version} is available. Open Settings → Connections to review it.`);
      } catch (error) { message = String(error); }
      finally { busy = false; paint(); }
  }
  settings.sync = () => {
    sync();
    const card = document.getElementById('native-updates');
    if (!card) return;
    card.querySelector('#update-auto').onchange = event => {
      const enabled = event.target.checked;
      try { localStorage.setItem('dm.check-updates', enabled?'1':'0'); autoCheck = enabled; }
      catch (_) { event.target.checked = autoCheck; message = 'This preference could not be saved. Manual checks are still available.'; paint(); return; }
      if (autoCheck) check();
    };
    card.querySelector('#update-check').onclick = () => check();
    card.querySelector('#update-install').onclick = async () => {
      if (busy || !candidate) return;
      const selected = candidate;
      if (!window.confirm(`Install version ${selected.version}? Save any unsaved work first. The application will close to install the update.`)) return;
      busy = true; message = 'Downloading and verifying the signed update…'; paint();
      try {
        await window.__TAURI__.core.invoke('update_install', {candidateId: selected.id});
        message = 'The installer was started.';
      } catch (error) { message = String(error); }
      finally { busy = false; paint(); }
    };
    paint();
  };
  const start = () => { if (autoCheck) check(true); };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start, {once:true});
  else setTimeout(start, 0);
})();
