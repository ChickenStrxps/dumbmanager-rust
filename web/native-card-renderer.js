/* Browser-side broker for the original DumbAPI card renderer. No network I/O. */
(()=>{
  'use strict';
  const base=new URL('.',document.currentScript.src);
  const MAX=15*1024*1024, MAX_QUEUED=4, TIMEOUT=120000;
  let queued=0,tail=Promise.resolve(),scriptPromise;
  function cancelled(signal){if(signal?.aborted)throw new DOMException('Card render cancelled.','AbortError');}
  function loadMain(){
    return scriptPromise??=(new Promise((resolve,reject)=>{
      if(window.DMCardGif){resolve();return;}
      const script=document.createElement('script');script.src=new URL('card-renderer.js',base).href;
      script.onload=()=>resolve();script.onerror=()=>reject(new Error('Cannot load the local card renderer.'));document.head.append(script);
    }));
  }
  async function inMain(payload,vibe,force,signal){
    await loadMain();cancelled(signal);
    const deadline=Date.now()+TIMEOUT;
    const result=await window.DMCardGif.make(payload,vibe,()=>!!signal?.aborted||Date.now()>deadline,force);
    cancelled(signal);
    if(!result)throw new Error('Card render timed out.');
    const {blob,...metadata}=result;if(blob.size>MAX)throw new Error('Animated card is larger than 15 MiB.');
    return {metadata,bytes:await blob.arrayBuffer()};
  }
  function pixelsHash(data){let a=2166136261,b=0x9e3779b9;for(let i=0;i<data.length;i++){a=Math.imul(a^data[i],16777619);b=Math.imul(b^data[i],2246822519);}return [a>>>0,b>>>0];}
  async function mainProbe(payload,vibe,force,signal){
    await loadMain();cancelled(signal);const scene=await window.DMCardGif.prepare(payload,vibe,force);
    try{scene.draw(0);return pixelsHash(scene.canvas.getContext('2d').getImageData(0,0,1200,675).data);}finally{scene.close();}
  }
  function inWorker(payload,vibe,force,signal,probe=false){
    return new Promise((resolve,reject)=>{
      const worker=new Worker(new URL('card-worker.js',base));let finished=false;
      const finish=(error,result)=>{if(finished)return;finished=true;clearTimeout(timer);signal?.removeEventListener('abort',abort);worker.terminate();error?reject(error):resolve(result);};
      const abort=()=>finish(new DOMException('Card render cancelled.','AbortError'));
      const timer=setTimeout(()=>finish(new Error('Card render timed out.')),TIMEOUT);
      signal?.addEventListener('abort',abort,{once:true});
      worker.onmessage=({data})=>data?.ok?finish(null,data):finish(new Error(data?.error||'Card render failed.'));
      worker.onerror=()=>finish(new Error('Card worker could not render this card.'));
      if(signal?.aborted){abort();return;}
      worker.postMessage({payload,vibe,force,probe});
    });
  }
  function encode(buffer){
    const bytes=new Uint8Array(buffer);if(!bytes.length||bytes.length>MAX)throw new Error('Invalid animated card size.');
    let raw='';for(let i=0;i<bytes.length;i+=32768)raw+=String.fromCharCode(...bytes.subarray(i,i+32768));
    return btoa(raw);
  }
  async function perform(args,callNative,signal){
    cancelled(signal);
    const prepared=await callNative('manager.social.gif_prepare',args);
    cancelled(signal);
    let result;
    if(typeof Worker==='function'&&typeof OffscreenCanvas==='function'){
      // Worker fonts can differ from the WebView's DOM canvas. Only use a
      // worker if the exact original first frame matches the DOM renderer.
      let same=false;
      try{const [main,worker]=await Promise.all([mainProbe(prepared.payload,prepared.vibe,prepared.force,signal),inWorker(prepared.payload,prepared.vibe,prepared.force,signal,true)]);same=main[0]===worker.probe[0]&&main[1]===worker.probe[1];}catch(error){if(signal?.aborted)throw error;}
      result=same?await inWorker(prepared.payload,prepared.vibe,prepared.force,signal):await inMain(prepared.payload,prepared.vibe,prepared.force,signal);
    }else result=await inMain(prepared.payload,prepared.vibe,prepared.force,signal);
    cancelled(signal);
    return callNative('manager.social.gif_store',{nonce:prepared.nonce,gif:encode(result.bytes),metadata:result.metadata});
  }
  function gif(args,callNative,options={}){
    if(typeof callNative!=='function')return Promise.reject(new TypeError('Native call is required.'));
    if(queued>=MAX_QUEUED)return Promise.reject(new Error('Too many card renders are queued.'));
    queued++;
    const run=tail.catch(()=>{}).then(()=>perform(args,callNative,options.signal));
    tail=run.finally(()=>{queued--;});tail.catch(()=>{});
    return run;
  }
  window.DMNativeCards=Object.freeze({gif});
})();
