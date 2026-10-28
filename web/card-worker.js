/* Runs the original canvas renderer in a single isolated WebView worker. */
'use strict';
self.window=self;
self.document={
  fonts:self.fonts||{load:()=>Promise.resolve([])},
  createElement(name){
    if(name!=='canvas')throw new Error('Unsupported renderer element');
    const canvas=new OffscreenCanvas(1,1);
    canvas.dataset={};
    return canvas;
  }
};
importScripts('card-renderer.js');
function pixelsHash(data){let a=2166136261,b=0x9e3779b9;for(let i=0;i<data.length;i++){a=Math.imul(a^data[i],16777619);b=Math.imul(b^data[i],2246822519);}return [a>>>0,b>>>0];}
let busy=false;
self.onmessage=async event=>{
  if(busy)return;
  busy=true;
  try{
    const {payload,vibe,force,probe}=event.data;
    if(probe){const scene=await self.DMCardGif.prepare(payload,vibe,force);try{scene.draw(0);const c=scene.canvas.getContext('2d');self.postMessage({ok:true,probe:pixelsHash(c.getImageData(0,0,1200,675).data)});}finally{scene.close();}return;}
    const result=await self.DMCardGif.make(payload,vibe,()=>false,force);
    if(!result||result.blob.size>15*1024*1024)throw new Error('Animated card is larger than 15 MiB.');
    const {blob,...metadata}=result;
    const bytes=await blob.arrayBuffer();
    self.postMessage({ok:true,metadata,bytes},[bytes]);
  }catch(error){self.postMessage({ok:false,error:String(error?.message||'Card render failed.')});}finally{busy=false;}
};
