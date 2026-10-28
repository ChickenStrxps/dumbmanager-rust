'use strict';
// Shortcuts only wake the existing UI. A send is never performed by this adapter.
(() => {
  let snipping = false;
  async function accept(result) {
    const snip = result.snip;
    if (!snip || snip.cancelled) { if (snip?.cancelled) toast('Snip cancelled.'); return; }
    if (snip.attached) {
      await refresh();
      if (selected === snip.itch_id && document.querySelector('#itch-view')?.open) {
        DMItches.decorate(); await DMItches.refreshMedia();
      }
      toast('Receipt added. Open it to review before sharing.');
    } else {
      DMUploads.restore('@itch', [snip.attachment]);
      if (!document.querySelector('#itch-view')?.open) {
        setView('now'); document.querySelector('#mode-itch')?.click();
      }
      toast(snip.error ? snip.error + ' The snip is kept in your stash draft.' : 'Snip added to your stash draft. Review it before saving or sharing.');
    }
  }
  async function snip(itchId = '') {
    if (snipping) return;
    snipping = true;
    try { await accept(await post('/api/native', {action:'snip', itch_id:itchId})); }
    catch (error) { toast(error.message); }
    finally { snipping = false; }
  }
  window.DMNativeDesktop = {accept, snip};
  async function install() {
    const listen = window.__TAURI__?.event?.listen;
    if (typeof listen !== 'function') return;
    await listen('desktop-shortcut', ({payload:id}) => {
      if (id === 5) { snip(document.querySelector('#itch-view')?.open ? selected : ''); return; }
      if (id === 4) {
        const dialog = document.querySelector('#itch-view');
        if (!dialog?.open) { if (typeof toast === 'function') toast('Open an itch and choose its thread first.'); return; }
        const button = document.querySelector('#itch-send-now');
        button?.focus();
        if (typeof toast === 'function') toast('Review the selected itch and thread, then use Send.');
        return;
      }
      if (document.querySelector('#itch-view')?.open) { if (typeof toast === 'function') toast('Finish the open itch first.'); return; }
      if (typeof setView === 'function') setView('now');
      document.querySelector('#mode-itch')?.click();
      if (id === 2) document.querySelector('#moment')?.click();
      else document.querySelector('#command')?.focus();
    });
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', () => { install().catch(() => {}); }, {once:true});
  else install().catch(() => {});
})();
