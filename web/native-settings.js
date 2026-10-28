'use strict';
// Small native controls in the original Connections page.
(() => {
  const t3 = window.DMT3;
  if (!t3) return;
  const markup = t3.markup.bind(t3), sync = t3.sync.bind(t3);
  t3.markup = () => markup() + `<details class="tool-card native-settings-card" id="native-local-tools"><summary>Video frameboards and Git folders</summary>
    <p>Choose your installed FFmpeg tools to create nine timestamped video frames. Files stay in this app. Saving these paths does not run the tools.</p>
    <label for="native-ffmpeg">FFmpeg executable</label><input id="native-ffmpeg" maxlength="1024" autocomplete="off">
    <label for="native-ffprobe">FFprobe executable</label><input id="native-ffprobe" maxlength="1024" autocomplete="off">
    <button type="button" class="secondary" id="native-video-save">Save video tools</button> <button type="button" class="quiet" id="native-video-clear">Disconnect video tools</button>
    <p>Git clones go into a folder you choose. Existing projects are registered above.</p>
    <label for="native-clone-root">Clone folder</label><input id="native-clone-root" maxlength="2000" autocomplete="off">
    <button type="button" class="secondary" id="native-git-save">Save clone folder</button><p id="native-local-status" role="status" aria-live="polite"></p></details>`;
  t3.sync = () => {
    sync();
    const card = document.querySelector('#native-local-tools');
    if (!card) return;
    const notice = card.querySelector('#native-local-status');
    let loaded = false;
    card.addEventListener('toggle', async () => {
      if (!card.open || loaded) return;
      try {
        const [video, git] = await Promise.all([call('manager.media.frameboard_settings'), call('git.settings')]);
        if (!card.isConnected) return;
        card.querySelector('#native-ffmpeg').value = video.ffmpeg || '';
        card.querySelector('#native-ffprobe').value = video.ffprobe || '';
        card.querySelector('#native-clone-root').value = git.clone_root || '';
        loaded = true;
      } catch (error) { notice.textContent = error.message; }
    });
    const save = (id, operation) => {
      card.querySelector(id).onclick = async event => {
        const button = event.currentTarget; button.disabled = true;
        try { await operation(); notice.textContent = 'Saved for this Rust installation.'; }
        catch (error) { notice.textContent = error.message; }
        finally { button.disabled = false; }
      };
    };
    save('#native-video-save', () => call('manager.media.frameboard_configure', {confirm: true,
      ffmpeg: card.querySelector('#native-ffmpeg').value, ffprobe: card.querySelector('#native-ffprobe').value}));
    save('#native-video-clear', async () => {
      await call('manager.media.frameboard_configure', {confirm: true, clear: true});
      card.querySelector('#native-ffmpeg').value = ''; card.querySelector('#native-ffprobe').value = '';
    });
    save('#native-git-save', () => call('git.configure', {clone_root: card.querySelector('#native-clone-root').value}));
  };
  const paths = {'native-t3-cli': false, 'native-t3-home': true, 'native-project-path': true,
    'native-ffmpeg': false, 'native-ffprobe': false, 'native-clone-root': true, 'native-browser-executable': false};
  function mount() {
    for (const [id, folder] of Object.entries(paths)) {
      const input = document.getElementById(id);
      if (!input || input.dataset.picker) continue;
      input.dataset.picker = 'true';
      const button = document.createElement('button'); button.type = 'button'; button.className = 'quiet';
      button.textContent = 'Choose…'; button.setAttribute('aria-label', `Choose ${input.labels?.[0]?.textContent || 'path'}`);
      button.onclick = async () => {
        button.disabled = true;
        try {
          const path = await window.__TAURI__.core.invoke('choose_path', {folder});
          if (path && input.isConnected) { input.value = path; input.dispatchEvent(new Event('input', {bubbles: true})); }
        } catch (error) { toast(typeof error === 'string' ? error : 'Could not open the file chooser.'); }
        finally { button.disabled = false; }
      };
      input.after(button);
    }
  }
  const init = () => {
    new MutationObserver(mount).observe(document.body, {childList: true, subtree: true}); mount();
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init, {once: true}); else init();
})();
