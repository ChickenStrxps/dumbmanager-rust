'use strict';
// A missing bundled entry point is an installation error. Never reload scripts:
// rerunning the original modules would duplicate their animation and UI clocks.
document.addEventListener('DOMContentLoaded',()=>{
  if(window.DMAppStarted)return;
  const dialog=document.createElement('dialog');dialog.id='entry-recovery';dialog.className='startup-recovery';
  const art=document.createElement('pre');art.setAttribute('aria-hidden','true');art.textContent='[ .. ]';
  const title=document.createElement('h2');title.textContent='App could not start';
  const message=document.createElement('p');message.setAttribute('role','status');message.textContent='A bundled app file is unavailable. Reinstall DumbManager Rust, then open it again.';
  const button=document.createElement('button');button.type='button';button.className='primary';button.textContent='Close';button.onclick=()=>dialog.close();
  dialog.append(art,title,message,button);document.body.append(dialog);dialog.showModal();
});
