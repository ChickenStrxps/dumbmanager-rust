'use strict';
// Native-only configuration in the existing Settings panel. No credentials enter the webview.
(() => {
  const original = window.DMT3;
  if (!original) return;
  const markup = original.markup.bind(original), sync = original.sync.bind(original);
  original.markup = () => markup() + `<details class="tool-card native-settings-card" id="native-t3-config"><summary>Local T3 and project folders</summary>
    <p>Choose the distribution above, then enter its CLI file, profile folder and local port. Save before connecting. No other installation is searched.</p>
    <label for="native-t3-cli">T3 CLI file</label><input id="native-t3-cli" type="text" maxlength="4096" autocomplete="off" placeholder="Absolute path to t3.cmd">
    <label for="native-t3-home">T3 profile folder</label><input id="native-t3-home" type="text" maxlength="4096" autocomplete="off" placeholder="Absolute profile folder">
    <label for="native-t3-port">Local port</label><input id="native-t3-port" type="number" min="1024" max="65535" value="3773">
    <button type="button" class="secondary" id="native-t3-save">Save connection details</button>
    <p>Register a project folder for project selection, Git and research. This only records the folder you specify.</p>
    <label for="native-project-id">Project ID</label><input id="native-project-id" maxlength="100" placeholder="letters, numbers, - or _">
    <label for="native-project-name">Project name</label><input id="native-project-name" maxlength="200">
    <label for="native-project-path">Project folder</label><input id="native-project-path" maxlength="4096" placeholder="Absolute folder path">
    <button type="button" class="secondary" id="native-project-add">Register project</button>
    <div id="native-project-list"></div><p id="native-config-status" role="status" aria-live="polite"></p></details>`;
  original.sync = () => {
    sync();
    const card = document.querySelector('#native-t3-config'); if (!card) return;
    const status = card.querySelector('#native-config-status');
    const distribution = () => document.querySelector('#t3-distribution')?.value || 't3';
    const fill = async () => {
      try {
        const data = await call('t3.distributions'); if (!card.isConnected) return;
        const row = data.items.find(item => item.id === distribution());
        card.querySelector('#native-t3-cli').value = row?.cli || '';
        card.querySelector('#native-t3-home').value = row?.home || '';
        card.querySelector('#native-t3-port').value = row?.port || (distribution() === 'knife' ? 4773 : 3773);
        const projects = await call('projects.list', {limit: 200}); if (!card.isConnected) return;
        const list = card.querySelector('#native-project-list'); list.replaceChildren();
        for (const project of projects.items) {
          const row = document.createElement('p');
          const label = document.createElement('span'); label.textContent = `${project.name} (${project.id}) · ${project.path}`;
          const button = document.createElement('button'); button.type = 'button'; button.className = 'quiet'; button.textContent = 'Remove';
          button.onclick = async () => { button.disabled = true; try { await call('projects.remove', {id:project.id}); status.textContent = 'Project registration removed; folder was not changed.'; await fill(); } catch (e) { status.textContent = e.message; button.disabled = false; } };
          row.append(label, ' ', button); list.append(row);
        }
      } catch (e) { if (card.isConnected) status.textContent = e.message; }
    };
    card.querySelector('#native-t3-save').onclick = async e => {
      const button = e.currentTarget; button.disabled = true;
      try { await call('t3.configure', {distribution:distribution(),cli:card.querySelector('#native-t3-cli').value,home:card.querySelector('#native-t3-home').value,port:Number(card.querySelector('#native-t3-port').value)}); status.textContent = 'Saved. Use Switch & connect to pair and verify.'; await original.load(); }
      catch (error) { status.textContent = error.message; } finally { button.disabled = false; }
    };
    card.querySelector('#native-project-add').onclick = async e => {
      const button = e.currentTarget; button.disabled = true;
      try { await call('projects.register', {id:card.querySelector('#native-project-id').value,name:card.querySelector('#native-project-name').value,path:card.querySelector('#native-project-path').value}); status.textContent = 'Project registered. Reload to update project selectors.'; await fill(); }
      catch (error) { status.textContent = error.message; } finally { button.disabled = false; }
    };
    fill();
  };
})();
