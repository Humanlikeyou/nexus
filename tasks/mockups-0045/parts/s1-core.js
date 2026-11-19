import * as THREE from 'three';
const $=(s,r=document)=>r.querySelector(s);
const DPR=Math.min(window.devicePixelRatio||1,2);
const REDUCED=matchMedia('(prefers-reduced-motion: reduce)').matches;
const clamp=(v,a,b)=>Math.max(a,Math.min(b,v));
const lerp=(a,b,t)=>a+(b-a)*t;
const ease=t=>1-Math.pow(1-clamp(t,0,1),3);
function rng(seed){let s=seed>>>0;return()=>{s=(s+0x6D2B79F5)>>>0;let t=s;t=Math.imul(t^t>>>15,t|1);t^=t+Math.imul(t^t>>>7,t|61);return((t^t>>>14)>>>0)/4294967296;}}
const fmt=(n,d=0)=>n.toLocaleString('en-US',{minimumFractionDigits:d,maximumFractionDigits:d});
const usd=n=>'$'+fmt(n);
const jpy=n=>'JPY '+fmt(n);
const RATE=150;
const hm=m=>`${Math.floor(m/60)} h ${String(Math.round(m%60)).padStart(2,'0')} min`;
const MON=['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'];
const DOW=['Sun','Mon','Tue','Wed','Thu','Fri','Sat'];
const END=new Date(2026,10,18); // last full day, synthetic
const dayAt=back=>{const d=new Date(END);d.setDate(d.getDate()-back);return d;};
const dlabel=d=>`${DOW[d.getDay()]} ${d.getDate()} ${MON[d.getMonth()]}`;
const sshort=d=>`${d.getDate()} ${MON[d.getMonth()]}`;

/* ---- theme ---- */
const T={};
function readTheme(){
  const cs=getComputedStyle(document.documentElement);
  const g=n=>cs.getPropertyValue(n).trim();
  T.dark=document.documentElement.dataset.theme==='dark';
  for(const k of ['bg','content','label','label2','label3','accent','accent2','copper']) T[k]=new THREE.Color(g('--c-'+k));
  T.deep=T.accent.clone();
  T.light=T.accent.clone().lerp(new THREE.Color(T.dark?0xffffff:0x9fe8cf),T.dark?.35:.45);
  T.stage={deep:T.accent.clone(),rem:T.accent.clone().lerp(T.content,.4),light:T.accent.clone().lerp(T.content,.74),awake:T.label3.clone().lerp(T.content,.45)};
  T.copperSoft=T.copper.clone().lerp(T.content,.45);
}
const mixc=(a,b,t)=>a.clone().lerp(b,t);

/* ---- shared renderer ---- */
const canvas=$('#gl');
const renderer=new THREE.WebGLRenderer({canvas,antialias:true,alpha:true,powerPreference:'high-performance'});
renderer.setPixelRatio(DPR);
renderer.autoClear=false;
renderer.setClearColor(0x000000,0);
function sizeRenderer(){renderer.setSize(window.innerWidth,window.innerHeight,false);canvas.style.width='100vw';canvas.style.height='100vh';}
sizeRenderer();

const UNIT=new THREE.PlaneGeometry(1,1);UNIT.userData.shared=true;
const CIRC=new THREE.CircleGeometry(1,40);CIRC.userData.shared=true;
let ORDER=0;
const mat2=(color,op=1,blend)=>new THREE.MeshBasicMaterial({color,transparent:true,opacity:op,side:THREE.DoubleSide,depthTest:false,depthWrite:false,blending:blend||THREE.NormalBlending});
const glowBlend=()=>T.dark?THREE.AdditiveBlending:THREE.NormalBlending;

class Panel{
  constructor(id,{persp=false,anim=true}={}){
    this.id=id;this.el=$(`[data-slot="${id}"]`);this.persp=persp;this.anim=anim;
    this.scene=new THREE.Scene();this.ov=new THREE.Scene();
    this.lbl=document.createElement('div');this.lbl.className='lbls';this.el.append(this.lbl);
    this.tipEl=document.createElement('div');this.tipEl.className='tip';this.el.append(this.tipEl);
    this.visible=true;this.t0=performance.now();this.hover=null;this.hoverMeshes=[];
    this.W=0;this.H=0;this.ready=false;
    this.band=new THREE.Mesh(UNIT,mat2(0xffffff,.1));this.band.visible=false;this.band.renderOrder=1e6;this.ov.add(this.band);
    this.hair=new THREE.Mesh(UNIT,mat2(0xffffff,.5));this.hair.visible=false;this.hair.renderOrder=1e6+1;this.ov.add(this.hair);
    this.dot=new THREE.Mesh(CIRC,mat2(0xffffff,1));this.dot.visible=false;this.dot.renderOrder=1e6+2;this.ov.add(this.dot);
    this.ovCam=new THREE.OrthographicCamera(0,1,0,1,-10,10);
    this.el.addEventListener('pointermove',e=>this.move(e));
    this.el.addEventListener('pointerleave',()=>this.leave());
    panels.push(this);
  }
  resize(){
    const r=this.el.getBoundingClientRect();if(r.width<10)return;
    this.W=r.width;this.H=r.height;
    this.ovCam.right=this.W;this.ovCam.bottom=this.H;this.ovCam.updateProjectionMatrix();
    this.rebuild();
  }
  clear(){
    const kill=o=>{o.traverse(c=>{if(c.geometry&&!c.geometry.userData.shared)c.geometry.dispose();if(c.material){(Array.isArray(c.material)?c.material:[c.material]).forEach(m=>m.dispose());}if(c.dispose&&c.isInstancedMesh)c.dispose();});};
    [...this.scene.children].forEach(c=>{this.scene.remove(c);kill(c);});
    this.lbl.innerHTML='';this.hoverMeshes=[];this.pulse=null;this.upd=null;
    ORDER=0;
  }
  rebuild(){
    if(!this.W)return;
    this.clear();this.leave();
    this.cam=this.persp?null:new THREE.OrthographicCamera(0,this.W,0,this.H,-1000,1000);
    this.build(T);this.ready=true;
  }
  add(o,scene){o.renderOrder=++ORDER;(scene||this.scene).add(o);return o;}
  rect(x,y,w,h,color,op=1,z=0){const m=new THREE.Mesh(UNIT,mat2(color,op));m.position.set(x+w/2,y+h/2,z);m.scale.set(w,h,1);return this.add(m);}
  circle(x,y,r,color,op=1){const m=new THREE.Mesh(CIRC,mat2(color,op));m.position.set(x,y,0);m.scale.set(r,r,1);return this.add(m);}
  rects(list,op=1){
    if(!list.length)return null;
    const m=new THREE.InstancedMesh(UNIT,mat2(0xffffff,op),list.length);const d=new THREE.Object3D();
    list.forEach((r,i)=>{d.position.set(r.x+r.w/2,r.y+r.h/2,0);d.scale.set(r.w,r.h,1);d.updateMatrix();m.setMatrixAt(i,d.matrix);m.setColorAt(i,r.c);});
    m.frustumCulled=false;return this.add(m);
  }
  hline(x0,x1,y,color,op=.2,h=1){return this.rect(x0,y-h/2,x1-x0,h,color,op);}
  vline(x,y0,y1,color,op=.2,w=1){return this.rect(x-w/2,y0,w,y1-y0,color,op);}
  dashed(x0,y0,x1,y1,color,op=.6,dash=5,gap=4,w=1.5){
    const L=Math.hypot(x1-x0,y1-y0),n=Math.max(1,Math.floor(L/(dash+gap))),pts=[];
    for(let i=0;i<n;i++){const a=i*(dash+gap)/L,b=Math.min(1,(i*(dash+gap)+dash)/L);pts.push([lerp(x0,x1,a),lerp(y0,y1,a),lerp(x0,x1,b),lerp(y0,y1,b)]);}
    const geo=segGeo(pts,w);const m=new THREE.Mesh(geo,mat2(color,op));return this.add(m);
  }
  ribbon(pts,o={}){const m=ribbonMesh(pts,o);return this.add(m);}
  area(top,bot,o={}){const m=areaMesh(top,bot,o);return this.add(m);}
  text(x,y,html,cls='',anchor='l'){
    const s=document.createElement('span');s.className=`lb a-${anchor} ${cls}`;s.innerHTML=html;s.style.left=x.toFixed(1)+'px';s.style.top=y.toFixed(1)+'px';this.lbl.append(s);return s;
  }
  // 3D helpers
  setupPersp({elev=40,yaw=0,fov=30,dist=1,target=[0,0,0],lights=true}={}){
    const cam=new THREE.PerspectiveCamera(fov,this.W/this.H,10,8000);
    const d=(this.H/2)/Math.tan(fov*Math.PI/360)*dist,e=elev*Math.PI/180,y=yaw*Math.PI/180;
    cam.position.set(target[0]+d*Math.cos(e)*Math.sin(y),target[1]+d*Math.sin(e),target[2]+d*Math.cos(e)*Math.cos(y));
    cam.lookAt(...target);cam.updateMatrixWorld();cam.updateProjectionMatrix();this.cam=cam;
    if(lights){
      this.scene.add(new THREE.HemisphereLight(0xffffff,T.dark?0x0b2a22:0xcfd6d2,T.dark?1.5:2.0));
      const dl=new THREE.DirectionalLight(0xffffff,T.dark?2.4:2.2);dl.position.set(-W_(this)*.4,this.H*1.2,this.H);this.scene.add(dl);this.key=dl;
    }
    return cam;
  }
  proj(x,y,z){const v=new THREE.Vector3(x,y,z).project(this.cam);return{x:(v.x+1)/2*this.W,y:(1-v.y)/2*this.H};}
  std(color,o={}){return new THREE.MeshStandardMaterial({color,roughness:.42,metalness:.08,emissive:color,emissiveIntensity:T.dark?.14:.06,...o});}
  // interaction
  move(e){
    if(!this.ready||!this.pick)return;
    const r=this.el.getBoundingClientRect();const x=e.clientX-r.left,y=e.clientY-r.top;
    const h=this.pick(x,y);this.show(h);
  }
  leave(){this.show(null);}
  show(h){
    if(this.onHover)this.onHover(h);
    if(!h){this.tipEl.style.display='none';this.band.visible=this.hair.visible=this.dot.visible=false;return;}
    const c=T.label;
    if(h.bx!=null){this.band.visible=true;this.band.material.color.copy(c);this.band.position.set(h.bx+h.bw/2,h.by+h.bh/2,0);this.band.scale.set(h.bw,h.bh,1);}else this.band.visible=false;
    if(h.hx!=null){this.hair.visible=true;this.hair.material.color.copy(c);this.hair.position.set(h.hx,h.hy0+(h.hy1-h.hy0)/2,0);this.hair.scale.set(1,h.hy1-h.hy0,1);}else this.hair.visible=false;
    if(h.px!=null){this.dot.visible=true;this.dot.material.color.copy(h.pc||T.accent);this.dot.position.set(h.px,h.py,0);this.dot.scale.set(4.5,4.5,1);}else this.dot.visible=false;
    const t=this.tipEl;t.innerHTML=h.tip;t.style.display='block';
    const tw=t.offsetWidth,th=t.offsetHeight;let tx=(h.ax??0)+12,ty=(h.ay??0)-th-10;
    if(tx+tw>this.W-4)tx=(h.ax??0)-tw-12;if(tx<4)tx=4;if(ty<4)ty=(h.ay??0)+16;if(ty+th>this.H-4)ty=this.H-th-4;
    t.style.left=tx+'px';t.style.top=ty+'px';
  }
}
function W_(p){return p.W}
const panels=[];

/* ---- geometry helpers ---- */
function segGeo(segs,w){ // segs [[x0,y0,x1,y1]] each a thin quad
  const pos=[],idx=[];
  segs.forEach((s,i)=>{const dx=s[2]-s[0],dy=s[3]-s[1],l=Math.hypot(dx,dy)||1,nx=-dy/l*w/2,ny=dx/l*w/2;
    pos.push(s[0]+nx,s[1]+ny,0,s[0]-nx,s[1]-ny,0,s[2]+nx,s[3]+ny,0,s[2]-nx,s[3]-ny,0);const b=i*4;idx.push(b,b+1,b+2,b+1,b+3,b+2);});
  const g=new THREE.BufferGeometry();g.setAttribute('position',new THREE.Float32BufferAttribute(pos,3));g.setIndex(idx);return g;
}
function ribbonMesh(pts,{w=1.5,color=0xffffff,op=1,cols=null,z=0,blend,depth=false}={}){
  const n=pts.length,pos=new Float32Array(n*6),idx=[];
  for(let i=0;i<n;i++){
    const p=pts[i],a=pts[Math.max(0,i-1)],b=pts[Math.min(n-1,i+1)];
    const dx=b.x-a.x,dy=b.y-a.y,l=Math.hypot(dx,dy)||1,nx=-dy/l*w,ny=dx/l*w;
    const ww=typeof w==='function'?w(i):1;
    pos.set([p.x+nx*ww,p.y+ny*ww,z,p.x-nx*ww,p.y-ny*ww,z],i*6);
    if(i<n-1){const k=i*2;idx.push(k,k+1,k+2,k+1,k+3,k+2);}
  }
  const g=new THREE.BufferGeometry();g.setAttribute('position',new THREE.BufferAttribute(pos,3));g.setIndex(idx);
  if(cols){const c=new Float32Array(n*6);for(let i=0;i<n;i++){const k=cols[i];c.set([k.r,k.g,k.b,k.r,k.g,k.b],i*6);}g.setAttribute('color',new THREE.BufferAttribute(c,3));}
  const m=new THREE.MeshBasicMaterial({color:cols?0xffffff:color,vertexColors:!!cols,transparent:true,opacity:op,side:THREE.DoubleSide,depthTest:depth,depthWrite:false,blending:blend||THREE.NormalBlending});
  return new THREE.Mesh(g,m);
}
function areaMesh(top,bot,{color=0xffffff,a0=.3,a1=0,depth=false,z=0}={}){
  const n=top.length,pos=new Float32Array(n*6),col=new Float32Array(n*8),idx=[];
  for(let i=0;i<n;i++){
    const b=Array.isArray(bot)?bot[i]:{x:top[i].x,y:bot};
    pos.set([top[i].x,top[i].y,z,b.x??top[i].x,b.y,z],i*6);
    col.set([color.r,color.g,color.b,a0,color.r,color.g,color.b,a1],i*8);
    if(i<n-1){const k=i*2;idx.push(k,k+1,k+2,k+1,k+3,k+2);}
  }
  const g=new THREE.BufferGeometry();g.setAttribute('position',new THREE.BufferAttribute(pos,3));g.setAttribute('color',new THREE.BufferAttribute(col,4));g.setIndex(idx);
  return new THREE.Mesh(g,new THREE.MeshBasicMaterial({vertexColors:true,transparent:true,side:THREE.DoubleSide,depthTest:depth,depthWrite:false}));
}
function ringPts(cx,cy,r,n,fn){const p=[];for(let i=0;i<=n;i++){const a=i/n*Math.PI*2;const rr=r+(fn?fn(a):0);p.push({x:cx+Math.cos(a)*rr,y:cy+Math.sin(a)*rr});}return p;}
function sectorShape(a0,a1,r0,r1){
  const s=new THREE.Shape();s.moveTo(r0*Math.cos(a0),r0*Math.sin(a0));s.lineTo(r1*Math.cos(a0),r1*Math.sin(a0));
  s.absarc(0,0,r1,a0,a1,false);s.lineTo(r0*Math.cos(a1),r0*Math.sin(a1));s.absarc(0,0,r0,a1,a0,true);return s;
}
const sectorGeo=(a0,a1,r0,r1)=>new THREE.ExtrudeGeometry(sectorShape(a0,a1,r0,r1),{depth:1,bevelEnabled:false,curveSegments:20});
function pointsMesh(n,size){
  const g=new THREE.BufferGeometry();g.setAttribute('position',new THREE.BufferAttribute(new Float32Array(n*3),3));g.setAttribute('color',new THREE.BufferAttribute(new Float32Array(n*4),4));
  const m=new THREE.PointsMaterial({size:size*DPR,sizeAttenuation:false,vertexColors:true,transparent:true,depthTest:false,depthWrite:false,blending:glowBlend()});
  const p=new THREE.Points(g,m);p.frustumCulled=false;return p;
}
function seg(label,id,cb){
  const root=$(id);root.addEventListener('click',e=>{const b=e.target.closest('button');if(!b)return;root.querySelectorAll('button').forEach(x=>x.setAttribute('aria-pressed',x===b));cb(+b.dataset.v);});
}
const arrowDelta=(z)=>z>=0?'above':'below';
function dbarHtml(z){const r=Math.min(Math.abs(z),2.5)/2.5*50;return `<div class="dbar"><i class="${z>=0?'r':'l'}" style="width:${r}%"></i></div>`;}

/* ---- main loop ---- */
let reg=[];
const io=new IntersectionObserver(es=>es.forEach(e=>{e.target._p.visible=e.isIntersecting;}),{rootMargin:'120px'});
const ro=new ResizeObserver(es=>es.forEach(e=>{const p=e.target._p;clearTimeout(p._rt);p._rt=setTimeout(()=>p.resize(),60);}));
let last=performance.now();
function frame(now){
  requestAnimationFrame(frame);
  const dt=Math.min(.05,(now-last)/1000);last=now;
  const vw=window.innerWidth,vh=window.innerHeight;
  renderer.setScissorTest(false);renderer.setViewport(0,0,vw,vh);renderer.clear();
  renderer.setScissorTest(true);
  for(const p of panels){
    if(!p.ready||!p.visible)continue;
    const r=p.el.getBoundingClientRect();
    if(r.bottom<0||r.top>vh||r.right<0||r.left>vw)continue;
    const y=vh-r.bottom;
    renderer.setViewport(r.left,y,r.width,r.height);renderer.setScissor(r.left,y,r.width,r.height);
    const t=(now-p.t0)/1000;
    if(p.upd)p.upd(t,dt);
    renderer.clear(false,true,false);
    renderer.render(p.scene,p.cam);
    renderer.clearDepth();
    renderer.render(p.ov,p.ovCam);
  }
}
function setTheme(name){
  document.documentElement.dataset.theme=name;
  $('#themeBtn').textContent='Theme: '+name;
  readTheme();
  panels.forEach(p=>{p.rebuild();});
  document.dispatchEvent(new Event('themechange'));
}
window.addEventListener('resize',()=>{sizeRenderer();});
function start(){
  const q=new URLSearchParams(location.search).get('theme');
  const pref=q||(matchMedia('(prefers-color-scheme: light)').matches?'light':'dark');
  document.documentElement.dataset.theme=pref;$('#themeBtn').textContent='Theme: '+pref;
  readTheme();
  panels.forEach(p=>{p.el._p=p;io.observe(p.el);ro.observe(p.el);p.resize();});
  $('#themeBtn').onclick=()=>setTheme(document.documentElement.dataset.theme==='dark'?'light':'dark');
  requestAnimationFrame(frame);
  window.__ready=true;
}
