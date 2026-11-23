/* ===== Panel 5: Sleep ===== */
const STG=['awake','rem','light','deep'];
const STN={awake:'Awake',rem:'REM',light:'Light',deep:'Deep'};
const SEQ=[['awake',6],['light',24],['deep',38],['light',16],['rem',10],['light',22],['deep',30],['light',18],['rem',18],['awake',4],['light',30],['deep',14],['light',20],['rem',26],['light',24],['rem',22],['light',30],['awake',5],['light',26],['rem',28],['light',20]];
const BED=23*60+12;
let acc=0;const NIGHT=SEQ.map(([s,m])=>{const o={s,m,t0:acc};acc+=m;return o;});
const NIGHT_TOTAL=acc;
const clock=m=>{m=((Math.round(m)%1440)+1440)%1440;return `${String(Math.floor(m/60)).padStart(2,'0')}:${String(m%60).padStart(2,'0')}`;};
const sleepHist=(()=>{const r=rng(55);const a=[];
  for(let i=89;i>=0;i--){const tot=i===0?NIGHT_TOTAL-NIGHT.filter(x=>x.s==='awake').reduce((s,x)=>s+x.m,0):clamp(445+38*Math.sin(i/5.5)+(r()-.5)*70+(i%7<2?-20:0),330,540);
    const deep=tot*(.15+r()*.06),rem=tot*(.2+r()*.06);a.push({back:i,tot,deep,rem,light:tot-deep-rem});}
  const l=a[a.length-1];const d=NIGHT.filter(x=>x.s==='deep').reduce((s,x)=>s+x.m,0),rm=NIGHT.filter(x=>x.s==='rem').reduce((s,x)=>s+x.m,0);l.deep=d;l.rem=rm;l.light=l.tot-d-rm;return a;})();
let sleepRange=7;
const sleepNights=()=>sleepHist.slice(-sleepRange);
$('#p5leg').innerHTML=['deep','rem','light','awake'].map(s=>`<span><s style="background:${{deep:'var(--c-accent)',rem:'color-mix(in srgb,var(--c-accent) 60%,var(--c-content))',light:'color-mix(in srgb,var(--c-accent) 26%,var(--c-content))',awake:'color-mix(in srgb,var(--c-label3) 55%,var(--c-content))'}[s]}"></s>${STN[s]}</span>`).join('')+'<span>Usual 7 to 8 h</span>';
const nightTip=n=>`<b>${dlabel(dayAt(n.back))}</b> · ${hm(n.tot)}<small>Deep ${hm(n.deep)} · REM ${hm(n.rem)} · Light ${hm(n.light)}</small>`;
const segTip=s=>`<b>${STN[s.s]}</b> · ${s.m} min<small>${clock(BED+s.t0)} to ${clock(BED+s.t0+s.m)}</small>`;
{
  const a=new Panel('p5a');
  a.build=function(){
    const W=this.W,H=this.H,L=44,R=10,pw=W-L-R;this.L=L;this.pw=pw;
    this.text(L-44,10,`<b class="t13">Last night</b> <i>${hm(NIGHT_TOTAL)} in bed · ${clock(BED)} to ${clock(BED+NIGHT_TOTAL)}</i>`,'t13','l');
    const hy=34,lane=26;this.hy=hy;this.lane=lane;
    STG.forEach((s,i)=>{this.text(L-8,hy+i*lane+11,STN[s],'l3','r');this.hline(L,L+pw,hy+i*lane+lane-2,T.label,.05);});
    const X=m=>L+m/NIGHT_TOTAL*pw;this.XS=X;
    const list=NIGHT.map(n=>({x:X(n.t0),y:hy+STG.indexOf(n.s)*lane,w:Math.max(2,n.m/NIGHT_TOTAL*pw-1),h:22,c:T.stage[n.s]}));
    this.rects(list);
    for(let i=0;i<NIGHT.length-1;i++){const n=NIGHT[i],m=NIGHT[i+1];const y0=hy+STG.indexOf(n.s)*lane+11,y1=hy+STG.indexOf(m.s)*lane+11;if(y0!==y1)this.vline(X(m.t0)-.5,Math.min(y0,y1),Math.max(y0,y1),T.label3,.5);}
    for(let h=0;h<=NIGHT_TOTAL;h+=120){this.text(X(h),hy+4*lane+8,clock(BED+h),'l3','c');}
    // nightly bars
    const by0=hy+4*lane+44,by1=H-34,bh=by1-by0;this.by0=by0;this.by1=by1;
    const ns=sleepNights(),N=ns.length,slot=pw/N,bw=Math.min(40,slot*.7);const Y=h=>by1-h/10*bh;this.YH=Y;
    this.text(L-44,by0-14,`<b class="t13">Nightly sleep</b> <i>last ${N} nights</i>`,'t13','l');
    for(const h of [0,4,8]){this.hline(L,L+pw,Y(h),T.label,.08);this.text(L-8,Y(h),h+' h','','r');}
    this.rect(L,Y(8),pw,Y(7)-Y(8),T.accent,T.dark?.16:.12);this.text(L+pw,Y(8)-8,'Usual 7 to 8 h','l3','br');
    const bars=ns.map((n,i)=>({x:L+slot*i+(slot-bw)/2,y:Y(n.tot/60),w:bw,h:by1-Y(n.tot/60),c:i===N-1?T.accent:mixc(T.accent,T.content,.5)}));
    this.rects(bars);this.bx=i=>L+slot*i+slot/2;this.slot=slot;this.ns=ns;
    const step=N<=7?1:N<=30?5:15;ns.forEach((n,i)=>{if((N-1-i)%step===0)this.text(L+slot*i+slot/2,by1+14,N<=7?DOW[dayAt(n.back).getDay()]:sshort(dayAt(n.back)),'l3','c');});
  };
  a.pick=function(x,y){
    if(y<this.hy+4*this.lane){const m=(x-this.L)/this.pw*NIGHT_TOTAL;const s=NIGHT.find(n=>m>=n.t0&&m<n.t0+n.m);if(!s)return null;
      const sx=this.XS(s.t0);return{tip:segTip(s),ax:x,ay:this.hy+STG.indexOf(s.s)*this.lane,bx:sx,by:this.hy+STG.indexOf(s.s)*this.lane,bw:s.m/NIGHT_TOTAL*this.pw,bh:22};}
    if(y>=this.by0-4){const i=clamp(Math.floor((x-this.L)/this.slot),0,this.ns.length-1);if(x<this.L)return null;const n=this.ns[i];
      return{tip:nightTip(n),ax:this.bx(i),ay:this.YH(n.tot/60),bx:this.L+this.slot*i,by:this.by0,bw:this.slot,bh:this.by1-this.by0};}
    return null;
  };

  const b=new Panel('p5b',{persp:true});
  b.build=function(){
    const W=this.W,H=this.H;this.setupPersp({elev:52,fov:28,dist:1.1,target:[0,30,-50]});
    const L=-W/2+44,pw=W-60;
    const ns=sleepNights(),N=ns.length,slot=pw/N,bw=Math.min(34,slot*.7);
    const floor=new THREE.Mesh(new THREE.BoxGeometry(W-10,3,420),new THREE.MeshStandardMaterial({color:T.label,transparent:true,opacity:T.dark?.06:.05,roughness:.2}));floor.position.set(0,-2,-20);this.scene.add(floor);
    // hypnogram boxes
    const lz=i=>-215+i*26,hgt={awake:5,rem:10,light:7,deep:14};
    const boxG=new THREE.BoxGeometry(1,1,1);
    const inst=new THREE.InstancedMesh(boxG,this.std(0xffffff,{emissive:0x000000,emissiveIntensity:0}),NIGHT.length);const d=new THREE.Object3D();this.segs=[];
    NIGHT.forEach((n,i)=>{const w=n.m/NIGHT_TOTAL*pw-1.5,x=L+n.t0/NIGHT_TOTAL*pw+w/2;const h=hgt[n.s];d.position.set(x,h/2,lz(STG.indexOf(n.s)));d.scale.set(w,h,21);d.updateMatrix();inst.setMatrixAt(i,d.matrix);inst.setColorAt(i,T.stage[n.s]);
      const p0=this.proj(x-w/2,0,lz(STG.indexOf(n.s))),p1=this.proj(x+w/2,0,lz(STG.indexOf(n.s)));this.segs.push({n,x0:p0.x,x1:p1.x,y:p0.y});});
    this.scene.add(inst);
    STG.forEach((s,i)=>{const p=this.proj(L-6,0,lz(i));this.text(p.x,p.y,STN[s],'l3','r');});
    this.text(4,this.proj(0,0,-250).y,`<b class="t13">Last night</b> <i>${hm(NIGHT_TOTAL)} · ${clock(BED)} to ${clock(BED+NIGHT_TOTAL)}</i>`,'t13','l');
    for(let h=0;h<=NIGHT_TOTAL;h+=120){const p=this.proj(L+h/NIGHT_TOTAL*pw,0,-215+4*26-4);this.text(p.x,p.y,clock(BED+h),'l3','c');}
    // bars: stacked
    const SC=15,bz=55;
    const items=[];ns.forEach((n,i)=>{let y=0;[['deep',n.deep],['rem',n.rem],['light',n.light]].forEach(([s,m])=>{const h=m/60*SC;items.push({x:L+slot*i+slot/2,y:y+h/2,h,s});y+=h;});});
    const bi=new THREE.InstancedMesh(boxG,this.std(0xffffff,{emissive:0x000000,emissiveIntensity:0}),items.length);
    items.forEach((it,i)=>{d.position.set(it.x,it.y,bz);d.scale.set(bw,Math.max(.5,it.h-.6),24);d.updateMatrix();bi.setMatrixAt(i,d.matrix);bi.setColorAt(i,T.stage[it.s]);});
    this.scene.add(bi);
    const band=new THREE.Mesh(new THREE.BoxGeometry(pw,SC,30),new THREE.MeshBasicMaterial({color:T.accent,transparent:true,opacity:T.dark?.2:.14,depthWrite:false}));band.position.set(L+pw/2,7.5*SC,bz);this.scene.add(band);
    const ed=new THREE.LineSegments(new THREE.EdgesGeometry(band.geometry),new THREE.LineBasicMaterial({color:T.accent,transparent:true,opacity:.6}));ed.position.copy(band.position);this.scene.add(ed);
    const q=this.proj(L+pw,8*SC,bz+15);
    for(const h of [0,4,8]){const p=this.proj(L-8,h*SC,bz+10);this.text(p.x,p.y,h+' h','','r');}
    this.text(4,H-10,`<b class="t13">Nightly sleep</b> <i>last ${N} nights · shaded band = usual 7 to 8 h</i>`,'t13','l');
    const step=N<=7?1:N<=30?5:15;this.nb=[];
    ns.forEach((n,i)=>{const x=L+slot*i+slot/2;const p=this.proj(x,0,bz+16);const top=this.proj(x,n.tot/60*SC,bz);this.nb.push({n,x:p.x,y:top.y});
      if((N-1-i)%step===0)this.text(p.x,p.y+10,N<=7?DOW[dayAt(n.back).getDay()]:sshort(dayAt(n.back)),'l3','c');});
    this.slotPx=this.nb.length>1?Math.abs(this.nb[1].x-this.nb[0].x):40;this.splitY=this.proj(0,0,-80).y;
    this.upd=()=>{if(this.key&&!REDUCED)this.key.position.x=-W*.3+Math.sin(performance.now()/2500)*W*.3;};
  };
  b.pick=function(x,y){
    if(y<this.splitY){const s=this.segs.find(s=>x>=s.x0&&x<=s.x1&&Math.abs(y-s.y)<26);return s?{tip:segTip(s.n),ax:x,ay:s.y}:null;}
    let best=null,bd=1e9;this.nb.forEach(o=>{const d=Math.abs(o.x-x);if(d<bd){bd=d;best=o;}});
    if(!best||bd>this.slotPx*.6)return null;return{tip:nightTip(best.n),ax:best.x,ay:best.y,hx:best.x,hy0:best.y,hy1:best.y+6};
  };
  seg('','#p5seg',v=>{sleepRange=v;a.rebuild();b.rebuild();});
}
