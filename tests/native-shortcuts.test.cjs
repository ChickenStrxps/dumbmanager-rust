'use strict';
const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync('web/app.js', 'utf8');
const captureSource = source.slice(source.indexOf('async function capture('), source.indexOf('function planMarkup('));
const dialogSource = source.slice(source.indexOf('function dialog('), source.indexOf('async function copy('));
function fixture(options = {}) {
  const elements = new Map();
  const el = id => {
    if (!elements.has(id)) elements.set(id, {value:'', open:false, disabled:false, focus(){this.focused=true;}, showModal(){this.open=true;}, close(){this.open=false;}});
    return elements.get(id);
  };
  el('#command').value = 'Existing draft';
  const calls = [], notes = [], commits = [], uploads = [{id:'existing-receipt'}];
  let counter = 0;
  const ctx = vm.createContext({
    $:el, saving:false, replayCaptureKey:'existing-draft-event', project:'fixture', selected:null,
    outbox:[], dialogSequence:0, Date, JSON,
    uid:()=>`request-${++counter}`, context:()=>null, window:{},
    DMUploads:{busy:false,itchAttachments:uploads,render(){},commit(...args){commits.push(args);}},
    updateSuggestions(){}, stashOutbox(){}, merge(){}, render(){},
    toast:message=>{notes.push(message);},
    replay:async(action,args)=>{calls.push(['replay',action,args]);return {event_token:'token'};},
    call:async(name,args)=>{
      calls.push([name,args]);
      if(options.fail) throw Object.assign(new Error('Storage unavailable'),{network:!!options.network});
      return {id:'saved-itch',version:7,title:args.title};
    },
    change:async(...args)=>{calls.push(['update',...args]);},
  });
  vm.runInContext(captureSource + dialogSource, ctx);
  return {ctx,el,calls,notes,commits,uploads,run:code=>vm.runInContext(code,ctx)};
}
test('stash shortcut opens focused popup without creating a record', async()=>{
  const f=fixture(); await f.run('quickCapture()');
  assert.equal(f.el('#editor').open,true);
  assert.equal(f.el('#quick-capture-note').focused,true);
  assert.equal(f.calls.length,0);
  f.el('#editor').close();
  assert.equal(f.el('#command').value,'Existing draft');
});
test('quick save keeps existing text, receipts and replay retry key separate',async()=>{
  const f=fixture(); await f.run('quickCapture()');
  f.el('#quick-capture-note').value='Separate itch'; await f.el('#dialog-save').onclick();
  const args=f.calls.find(c=>c[0]==='manager.capture')[1];
  assert.equal(args.title,'Separate itch'); assert.equal(args.attachments,'[]');
  assert.notEqual(args.request_key,'existing-draft-event');
  assert.equal(f.ctx.replayCaptureKey,'existing-draft-event');
  assert.equal(f.el('#command').value,'Existing draft'); assert.equal(f.commits.length,0);
  assert.equal(f.el('#editor').open,false);
});
test('empty quick input stays open and does not save',async()=>{
  const f=fixture(); await f.run('quickCapture()'); await f.el('#dialog-save').onclick();
  assert.equal(f.calls.length,0); assert.equal(f.el('#editor').open,true);
});
test('failed quick save retains popup and input for retry',async()=>{
  const f=fixture({fail:true}); await f.run('quickCapture()');
  f.el('#quick-capture-note').value='Retry me'; await f.el('#dialog-save').onclick();
  assert.equal(f.el('#editor').open,true); assert.equal(f.el('#quick-capture-note').value,'Retry me');
  assert.equal(f.el('#command').value,'Existing draft');
  assert.ok(f.notes.includes('Storage unavailable'));
});
test('offline quick save is retained in the existing retry outbox',async()=>{
  const f=fixture({fail:true,network:true}); await f.run('quickCapture()');
  f.el('#quick-capture-note').value='Queued itch'; await f.el('#dialog-save').onclick();
  assert.equal(f.ctx.outbox.length,1); assert.equal(f.ctx.outbox[0].title,'Queued itch');
  assert.equal(f.el('#editor').open,false); assert.equal(f.el('#command').value,'Existing draft');
});
test('moment is saved before optional note and closing does not undo it',async()=>{
  const f=fixture(); await f.run('quickCapture(true)');
  const saved=f.calls.filter(c=>c[0]==='manager.capture');
  assert.equal(saved.length,1); assert.equal(saved[0][1].moment,true);
  assert.equal(f.el('#editor').open,true); f.el('#editor').close();
  assert.equal(f.calls.filter(c=>c[0]==='update').length,0);
  assert.equal(f.el('#command').value,'Existing draft'); assert.equal(f.commits.length,0);
});
test('moment note updates exactly the new bookmark with version checking',async()=>{
  const f=fixture(); await f.run('quickCapture(true)');
  f.el('#quick-capture-note').value='A useful note'; await f.el('#dialog-save').onclick();
  const update=f.calls.find(c=>c[0]==='update');
  assert.equal(update[1],'saved-itch'); assert.equal(update[2].title,'A useful note'); assert.equal(update[3],7);
  assert.equal(f.calls.filter(c=>c[0]==='manager.capture').length,1);
});
test('shortcut preserves existing editors and open itch detail',async()=>{
  for(const id of ['#editor','#itch-view']) {
    const f=fixture(); f.el(id).open=true; await f.run('quickCapture(true)');
    assert.equal(f.calls.length,0); assert.equal(f.el(id).open,true);
  }
});
test('moment persistence cannot replace a dialog opened while saving',async()=>{
  for(const id of ['#editor','#itch-view']) {
    const f=fixture(); let finish;
    f.ctx.call=async()=>new Promise(resolve=>{finish=resolve;});
    const pending=f.run('quickCapture(true)');
    await new Promise(resolve=>setImmediate(resolve));
    f.el(id).open=true; f.el('#dialog-title').textContent='Existing editor';
    f.ctx.selected='user-opened-itch';
    finish({id:'saved-moment',version:1}); await pending;
    assert.equal(f.el(id).open,true); assert.equal(f.el('#dialog-title').textContent,'Existing editor');
    if(id==='#itch-view')assert.equal(f.ctx.selected,'user-opened-itch');
    assert.ok(f.notes.includes('Moment saved. Open it from the itch pile to add a note.'));
  }
});
test('main composer still commits its text and receipts',async()=>{
  const f=fixture(); await f.run('capture(false)');
  const args=f.calls.find(c=>c[0]==='manager.capture')[1];
  assert.equal(args.title,'Existing draft'); assert.equal(args.attachments,'["existing-receipt"]');
  assert.equal(args.request_key,'existing-draft-event');
  assert.equal(f.commits.length,1); assert.equal(f.el('#command').value,'');
});
function adapterFixture() {
  const events={}, elements=new Map(), actions=[];
  const ctx=vm.createContext({
    window:{__TAURI__:{event:{listen:async(name,handler)=>{events[name]=handler;}}}},
    document:{readyState:'complete',querySelector:selector=>elements.get(selector)},
    quickCapture:async moment=>{actions.push(['capture',moment]);},
    setView:view=>actions.push(['view',view]),toast:message=>actions.push(['toast',message]),
    selected:'open-itch',
    post:async(path,args)=>{actions.push(['post',path,args]);return {snip:{cancelled:true}};},
  });
  vm.runInContext(fs.readFileSync('web/desktop-native.js','utf8'),ctx);
  return {events,elements,actions};
}
test('native stash and moment events use the popup flow',()=>{
  const f=adapterFixture(); f.events['desktop-shortcut']({payload:1}); f.events['desktop-shortcut']({payload:2});
  assert.deepEqual(f.actions,[['capture',false],['capture',true]]);
});
test('cook only focuses review control; it never clicks or sends',()=>{
  const f=adapterFixture(); f.elements.set('#itch-view',{open:true});
  let focused=false; f.elements.set('#itch-send-now',{focus(){focused=true;},click(){assert.fail('must not send');}});
  f.events['desktop-shortcut']({payload:4}); assert.equal(focused,true);
  assert.equal(f.actions.filter(c=>c[0]==='post').length,0);
});
test('snip uses the open itch only when its detail is open',async()=>{
  for(const open of [false,true]) {
    const f=adapterFixture(); f.elements.set('#itch-view',{open});
    f.events['desktop-shortcut']({payload:5}); await new Promise(resolve=>setImmediate(resolve));
    assert.equal(f.actions[0][2].itch_id,open?'open-itch':'');
    assert.equal(f.actions.filter(c=>c[0]==='view').length,0);
  }
});
