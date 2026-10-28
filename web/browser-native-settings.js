'use strict';
// Extend the existing Connections panel without starting or discovering a browser.
(() => {
  const component = window.DMT3;
  if (!component) return;
  const markup = component.markup.bind(component), sync = component.sync.bind(component);
  component.markup = () => markup() + `<details class="tool-card native-settings-card" id="native-browser-config"><summary>Browse browser</summary><p>Choose an installed Chromium or Microsoft Edge executable. Browse starts it only when requested, using a separate temporary profile. Saving does not test the executable.</p><label for="native-browser-path">Browser executable</label><input id="native-browser-path" maxlength="4096" autocomplete="off" placeholder="Choose browser executable"><button type="button" class="secondary" id="native-browser-choose">Choose file</button> <button type="button" class="secondary" id="native-browser-save">Save</button><p id="native-browser-status" role="status" aria-live="polite"></p></details>`;
  component.sync = () => {
    sync();
    const card=document.querySelector('#native-browser-config'); if(!card)return;
    const field=card.querySelector('#native-browser-path'), note=card.querySelector('#native-browser-status');
    call('browser.status').then(result=>{if(card.isConnected){field.value=result.executable||'';note.textContent=result.configured?'Configured; start Browse to verify.':'No browser configured.';}}).catch(e=>{if(card.isConnected)note.textContent=e.message;});
    card.querySelector('#native-browser-choose').onclick=async()=>{try{const path=await window.__TAURI__.core.invoke('choose_path',{folder:false});if(path)field.value=path;}catch(e){note.textContent=e.message||String(e);}};
    card.querySelector('#native-browser-save').onclick=async event=>{const button=event.currentTarget;button.disabled=true;try{const result=await call('browser.configure',{executable:field.value.trim()});note.textContent=result.note;}catch(e){note.textContent=e.message;}finally{button.disabled=false;}};
  };
})();
