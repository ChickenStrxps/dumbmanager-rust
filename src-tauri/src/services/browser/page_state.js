(() => {
  const store = globalThis.__dmBrowse ||= {nonce: Array.from(crypto.getRandomValues(new Uint32Array(4))).join('-'), nodes: new Map(), ids: new WeakMap(), next: 0};
  const visible = el => {const r=el.getBoundingClientRect(),s=getComputedStyle(el);return r.width>0&&r.height>0&&r.bottom>0&&r.right>0&&r.top<innerHeight&&r.left<innerWidth&&s.visibility==='visible'&&s.display!=='none';};
  store.nodes.clear(); const nodes=[];
  const labelText = label => {if(!label)return '';const copy=label.cloneNode(true);copy.querySelectorAll('input,textarea,select,button,[contenteditable]').forEach(n=>n.remove());return copy.textContent;};
  for(const el of document.querySelectorAll('a[href],button,input,textarea,select,[contenteditable="true"],[role="textbox"],[role="searchbox"],[role="button"],[role="link"],[tabindex="0"]')){
    if(nodes.length>=80)break;
    if(!visible(el)||el.disabled||el.type==='password'||el.type==='hidden')continue;
    let id=store.ids.get(el);if(!id){id='n'+(++store.next);store.ids.set(el,id);}store.nodes.set(id,el);
    const r=el.getBoundingClientRect(),kind=el.tagName.toLowerCase();
    const label=(el.getAttribute('aria-label')||el.getAttribute('placeholder')||labelText(el.labels?.[0])||(kind==='input'||kind==='textarea'||el.isContentEditable?'':el.innerText)||el.getAttribute('title')||kind).replace(/\s+/g,' ').trim().slice(0,160);
    const editable=!el.readOnly&&(kind==='textarea'||el.isContentEditable||(kind==='input'&&['text','search','email','url','tel'].includes(el.type)));
    const options=kind==='select'?Array.from(el.options).filter(o=>!o.disabled).slice(0,80).map(o=>({value:o.value.slice(0,200),label:o.text.slice(0,160)})):undefined;
    nodes.push({id,kind,label,editable,input_type:kind==='input'?el.type:'',checked:['checkbox','radio'].includes(el.type)?el.checked:undefined,options,x:Math.round(Math.max(0,r.left)+(Math.min(innerWidth,r.right)-Math.max(0,r.left))/2),y:Math.round(Math.max(0,r.top)+(Math.min(innerHeight,r.bottom)-Math.max(0,r.top))/2)});
  }
  store.contentIds ||= new WeakMap();store.contentNext ||= 0;
  const parts=[], groups=new Map(), walker=document.createTreeWalker(document.body||document.documentElement,NodeFilter.SHOW_TEXT);let node,count=0,size=0;
  while((node=walker.nextNode())&&count++<4000&&size<8000){
    const p=node.parentElement;if(!p||p.closest('script,style,noscript,input,textarea,[contenteditable]')||!visible(p))continue;
    const range=document.createRange();range.selectNodeContents(node);
    const rects=Array.from(range.getClientRects()).filter(r=>r.width>0&&r.height>0&&r.bottom>0&&r.right>0&&r.top<innerHeight&&r.left<innerWidth);
    if(!rects.length)continue;
    const text=node.textContent.replace(/\s+/g,' ').trim().slice(0,8000-size);if(!text)continue;
    parts.push(text);size+=text.length;
    const block=p.closest('p,li,blockquote,pre,h1,h2,h3,h4,h5,h6,td,th')||p;
    if(!groups.has(block)&&groups.size>=48)continue;
    if(!groups.has(block)){let id=store.contentIds.get(block);if(!id){id='p'+(++store.contentNext);store.contentIds.set(block,id);}groups.set(block,{id,text:'',left:innerWidth,top:innerHeight,right:0,bottom:0});}
    const group=groups.get(block);group.text=(group.text+' '+text).trim().slice(0,900);
    for(const r of rects){group.left=Math.min(group.left,Math.max(0,r.left));group.top=Math.min(group.top,Math.max(0,r.top));group.right=Math.max(group.right,Math.min(innerWidth,r.right));group.bottom=Math.max(group.bottom,Math.min(innerHeight,r.bottom));}
  }
  const passages=Array.from(groups.values(),g=>({id:g.id,text:g.text,bounds:{x:Math.floor(g.left),y:Math.floor(g.top),width:Math.ceil(g.right-g.left),height:Math.ceil(g.bottom-g.top)}}));
  return {url:location.href,title:document.title.slice(0,250),nonce:store.nonce,text:parts.join(' ').slice(0,8000),passages,nodes,scrollY:Math.round(scrollY),height:innerHeight,width:innerWidth};
})()