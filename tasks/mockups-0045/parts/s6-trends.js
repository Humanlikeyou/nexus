/* ===== Panel 6: Trends ===== */
const LEN=365+28;
const MET=[
  {k:'Recovery',u:'%',m:64,s:11,phi:.7,dec:0,min:3,max:99,v:rec=>Math.round(rec)},
  {k:'HRV',u:'ms',m:62,s:5,phi:.8,dec:0,min:25,max:120},
  {k:'Resting HR',u:'bpm',m:55,s:1.6,phi:.75,dec:0,min:40,max:80},
  {k:'Strain',u:'/100',m:48,s:12,phi:.5,dec:0,min:0,max:100}];
MET.forEach((M,mi)=>{const r=rng(100+mi);let x=M.m;M.d=[];
  for(let i=0;i<LEN;i++){x=M.m+M.phi*(x-M.m)+M.s*Math.sqrt(1-M.phi**2)*(r()*2-1)*1.5+Math.sin(i/58+mi)*M.s*.12;M.d.push(clamp(x,M.min,M.max));}
  // make the last days stand out
  if(mi===1){M.d[LEN-1]=M.m+M.s*2.0;M.d[LEN-2]=M.m+M.s*1.4;}
  if(mi===2){M.d[LEN-1]=M.m-2.8;}
  if(mi===3){M.d[LEN-1]=M.m+M.s*1.6;}
  M.mu=[];M.sd=[];
  for(let i=0;i<LEN;i++){const a=Math.max(0,i-27);let s=0,q=0,n=0;for(let j=a;j<=i;j++){s+=M.d[j];n++;}const mu=s/n;for(let j=a;j<=i;j++)q+=(M.d[j]-mu)**2;M.mu.push(mu);M.sd.push(Math.max(.5,Math.sqrt(q/n)));}
});
let trRange=7;
const mval=(M,i)=>`${Math.round(M.d[i]*10)/10}${M.u==='%'?'%':' '+M.u}`;
function trSeries(M,N){const a=LEN-N;const idx=[];for(let i=a;i<LEN;i++)idx.push(i);
  let lo=1e9,hi=-1e9;idx.forEach(i=>{lo=Math.min(lo,M.d[i],M.mu[i]-M.sd[i]);hi=Math.max(hi,M.d[i],M.mu[i]+M.sd[i]);});const pad=(hi-lo)*.1+.5;return{idx,lo:lo-pad,hi:hi+pad};}
const outside=(M,i)=>Math.abs(M.d[i]-M.mu[i])>M.sd[i];
const trTip=(M,i)=>`<b>${M.k}</b> ${mval(M,i)} · ${dlabel(dayAt(LEN-1-i))}<small>Usual ${Math.round(M.mu[i]-M.sd[i])} to ${Math.round(M.mu[i]+M.sd[i])} · ${outside(M,i)?(M.d[i]>M.mu[i]?'above':'below')+' usual':'inside usual'}</small>`;
{
  const a=new Panel('p6a');
  a.build=function(){
    const W=this.W,H=this.H,cw=(W-16)/2,ch=(H-10)/2;this.cells=[];
    MET.forEach((M,mi)=>{
      const ox=(mi%2)*(cw+16),oy=Math.floor(mi/2)*(ch+10);
      const ser=trSeries(M,trRange),N=ser.idx.length;
      const px0=ox+34,px1=ox+cw-4,py0=oy+34,py1=oy+ch-20;
      const X=k=>px0+(N===1?0:k/(N-1))*(px1-px0),Y=v=>py1-(v-ser.lo)/(ser.hi-ser.lo)*(py1-py0);
      const last=LEN-1;
      this.text(ox,oy+9,`<b class="t13">${M.k}</b>`,'t13','l');
      this.text(ox+cw,oy+9,`<b class="t17" style="font-weight:400">${mval(M,last)}</b>`,'t17','r');
      this.text(ox,oy+25,outside(M,last)?(M.d[last]>M.mu[last]?'Above your usual':'Below your usual'):'Within your usual',outside(M,last)?'dn':'l3','l');
      this.hline(px0,px1,py1,T.label,.1);
      this.text(px0-5,Y(ser.hi-(ser.hi-ser.lo)*.1),Math.round(ser.hi),'l3','r');this.text(px0-5,Y(ser.lo+(ser.hi-ser.lo)*.1),Math.round(ser.lo),'l3','r');
      const top=ser.idx.map((i,k)=>({x:X(k),y:Y(M.mu[i]+M.sd[i])})),bot=ser.idx.map((i,k)=>({x:X(k),y:Y(M.mu[i]-M.sd[i])}));
      this.area(top,bot,{color:T.accent,a0:T.dark?.22:.16,a1:T.dark?.22:.16});
      const pts=ser.idx.map((i,k)=>({x:X(k),y:Y(M.d[i])}));const cols=ser.idx.map(i=>outside(M,i)?T.copper.clone():T.accent.clone().lerp(T.label,T.dark?.1:0));
      if(N>2){this.ribbon(pts,{w:T.dark?4:3.5,cols:cols.map(c=>c.clone()),op:.18,blend:glowBlend()});this.ribbon(pts,{w:N>100?.8:1.5,cols});}
      const lp=pts[N-1];this.circle(lp.x,lp.y,3.6,outside(M,last)?T.copper:T.accent,1);
      this.text(px0,py1+11,sshort(dayAt(LEN-1-ser.idx[0])),'l3','l');this.text(px1,py1+11,'today','l3','r');
      this.cells.push({M,ser,N,X,Y,ox,oy,cw,ch,px0,px1,py0,py1});
    });
  };
  a.pick=function(x,y){
    const c=this.cells.find(c=>x>=c.ox&&x<=c.ox+c.cw&&y>=c.oy&&y<=c.oy+c.ch);if(!c||x<c.px0||x>c.px1)return null;
    const k=clamp(Math.round((x-c.px0)/(c.px1-c.px0)*(c.N-1)),0,c.N-1),i=c.ser.idx[k];
    return{tip:trTip(c.M,i),ax:c.X(k),ay:c.Y(c.M.d[i]),hx:c.X(k),hy0:c.py0,hy1:c.py1,px:c.X(k),py:c.Y(c.M.d[i]),pc:outside(c.M,i)?T.copper:T.accent};
  };

  const b=new Panel('p6b',{persp:true});
  b.build=function(){
    const W=this.W,H=this.H;this.setupPersp({elev:16,yaw:-20,fov:28,dist:1.12,target:[0,6,0]});
    const pw=W*.6,ph=70,gap=112;this.layers=[];this.hit=[];
    MET.forEach((M,mi)=>{
      const yc=(1.5-mi)*gap,z=(mi-1.5)*-38;const ser=trSeries(M,trRange),N=ser.idx.length;
      const X=k=>-pw/2+(N===1?0:k/(N-1))*pw,Y=v=>yc-ph/2+(v-ser.lo)/(ser.hi-ser.lo)*ph;
      const g=new THREE.Group();g.position.z=z;this.scene.add(g);
      const slab=new THREE.Mesh(new THREE.PlaneGeometry(pw+16,ph+14),new THREE.MeshBasicMaterial({color:T.label,transparent:true,opacity:T.dark?.07:.045,side:2,depthWrite:false}));slab.position.set(0,yc,-1);g.add(slab);
      const hw=(pw+16)/2,hh=(ph+14)/2;const edge=new THREE.LineLoop(new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(-hw,-hh,0),new THREE.Vector3(hw,-hh,0),new THREE.Vector3(hw,hh,0),new THREE.Vector3(-hw,hh,0)]),new THREE.LineBasicMaterial({color:T.label,transparent:true,opacity:.28}));edge.position.copy(slab.position);g.add(edge);
      const top=ser.idx.map((i,k)=>({x:X(k),y:Y(M.mu[i]+M.sd[i])})),bot=ser.idx.map((i,k)=>({x:X(k),y:Y(M.mu[i]-M.sd[i])}));
      const am=areaMesh(top,bot,{color:T.accent,a0:T.dark?.3:.22,a1:T.dark?.3:.22,depth:false});g.add(am);
      const pts=ser.idx.map((i,k)=>({x:X(k),y:Y(M.d[i])}));const cols=ser.idx.map(i=>outside(M,i)?T.copper.clone():T.accent.clone().lerp(T.label,T.dark?.1:0));
      if(N>2){const gl=ribbonMesh(pts,{w:4,cols:cols.map(c=>c.clone()),op:.2,blend:glowBlend(),z:2});g.add(gl);const ln=ribbonMesh(pts,{w:N>100?.8:1.5,cols,z:3});g.add(ln);}
      const lp=pts[N-1];const dt=new THREE.Mesh(new THREE.SphereGeometry(4,16,12),new THREE.MeshBasicMaterial({color:outside(M,LEN-1)?T.copper:T.accent}));dt.position.set(lp.x,lp.y,4);g.add(dt);
      const l=this.proj(-pw/2-8,yc+ph/2+16,z),r=this.proj(pw/2+8,yc+ph/2+16,z);
      this.text(l.x,l.y,`<b class="t13">${M.k}</b>`,'t13','l');
      this.text(r.x,r.y,`<b class="t13" style="font-weight:400">${mval(M,LEN-1)}</b>`,'t13','r');
      const hp=new THREE.Mesh(new THREE.PlaneGeometry(pw,ph+14),new THREE.MeshBasicMaterial({visible:false}));hp.position.set(0,yc,0);g.add(hp);hp.userData={M,ser,N,yc,z,pw,ph,Yf:Y};this.hit.push(hp);
    });
    this.upd=(t)=>{this.scene.children.forEach((c,i)=>{if(c.isGroup&&!REDUCED)c.position.y=Math.sin(t*.8+i)*1.5;});};
  };
  b.pick=function(x,y){
    const rc=new THREE.Raycaster();rc.setFromCamera(new THREE.Vector2(x/this.W*2-1,-(y/this.H*2-1)),this.cam);
    const h=rc.intersectObjects(this.hit,false)[0];if(!h)return null;
    const u=h.object.userData;const lx=h.object.worldToLocal(h.point.clone()).x;
    const k=clamp(Math.round((lx+u.pw/2)/u.pw*(u.N-1)),0,u.N-1),i=u.ser.idx[k];
    const wx=-u.pw/2+k/(u.N-1||1)*u.pw;
    const p0=this.proj(wx,u.yc-u.ph/2,u.z),p1=this.proj(wx,u.yc+u.ph/2,u.z);
    return{tip:trTip(u.M,i),ax:p0.x,ay:p1.y,hx:p0.x,hy0:p1.y,hy1:p0.y};
  };
  seg('','#p6seg',v=>{trRange=v;a.rebuild();b.rebuild();});
}

/* ===== Panel 7: Training load ===== */
const SD=(()=>{const r=rng(77);const a=[];for(let i=0;i<56;i++){const dow=i%7;const base=[44,70,52,78,38,62,26][dow];a.push(Math.round(clamp(base+(r()-.5)*26+(i>40?7:0),8,96)));}return a;})();
const TL=(()=>{const o=[];for(let i=28;i<56;i++){const a7=SD.slice(i-6,i+1).reduce((s,v)=>s+v,0)/7,a28=SD.slice(i-27,i+1).reduce((s,v)=>s+v,0)/28;o.push({v:SD[i],a7,a28,back:55-i});}return o;})();
const RATIO=TL[27].a7/TL[27].a28;
const ratioTxt=RATIO>=.8&&RATIO<=1.3?'in range':RATIO>1.3?'above range':'below range';
const tlTip=d=>`<b>Strain ${d.v}</b> · ${dlabel(dayAt(d.back))}<small>7-day ${d.a7.toFixed(0)} · 28-day ${d.a28.toFixed(0)}</small>`;
{
  const a=new Panel('p7a');
  a.build=function(){
    const W=this.W,H=this.H,L=36,R=10,pw=W-L-R,py0=64,py1=H-26,ph=py1-py0;
    const slot=pw/28,bw=slot*.64;const Y=v=>py1-v/100*ph;this.Y=Y;this.L=L;this.slot=slot;this.py0=py0;this.py1=py1;
    this.text(0,10,`<b class="t17" style="font-weight:400">${TL[27].a7.toFixed(0)}</b> <i>7-day avg</i> &nbsp; <b class="t17" style="font-weight:400;color:var(--c-copper-text)">${TL[27].a28.toFixed(0)}</b> <i>28-day avg</i>`,'t13','l');
    // ratio track
    const tx0=W-250,tx1=W-8,Rr=v=>tx0+(v-.5)/1.3*(tx1-tx0);
    this.text(tx1,10,`Ratio <b>${RATIO.toFixed(2)}</b> · ${ratioTxt}`,'t13','r');
    this.rect(tx0,33,tx1-tx0,6,T.label,.1);this.rect(Rr(.8),33,Rr(1.3)-Rr(.8),6,T.accent,.5);
    this.rect(Rr(RATIO)-1.5,28,3,16,T.label,1);
    this.text(Rr(.8),52,'0.8','l3','c');this.text(Rr(1.3),52,'1.3','l3','c');this.text(Rr(.5),52,'0.5','l3','l');this.text(Rr(1.8),52,'1.8','l3','r');
    for(const v of [0,25,50,75,100]){this.hline(L,L+pw,Y(v),T.label,v===0?.14:.06);this.text(L-6,Y(v),v,'l3','r');}
    this.rects(TL.map((d,i)=>({x:L+slot*i+(slot-bw)/2,y:Y(d.v),w:bw,h:py1-Y(d.v),c:d.v>=75?T.copper:mixc(T.accent,T.content,.38)})));
    const p7=TL.map((d,i)=>({x:L+slot*i+slot/2,y:Y(d.a7)})),p28=TL.map((d,i)=>({x:L+slot*i+slot/2,y:Y(d.a28)}));
    this.ribbon(p7,{w:4.5,color:T.label,op:.12});this.ribbon(p7,{w:1.5,color:T.label});
    for(let i=0;i<27;i++)this.dashed(p28[i].x,p28[i].y,p28[i+1].x,p28[i+1].y,T.copper,1,5,3,1.8);
    [0,7,14,21,27].forEach(i=>this.text(L+slot*i+slot/2,py1+13,i===27?'today':sshort(dayAt(TL[i].back)),'l3',i===0?'l':i===27?'r':'c'));
  };
  a.pick=function(x,y){const i=Math.floor((x-this.L)/this.slot);if(i<0||i>27)return null;const d=TL[i];
    return{tip:tlTip(d),ax:this.L+this.slot*(i+.5),ay:this.Y(d.v),bx:this.L+this.slot*i,by:this.py0,bw:this.slot,bh:this.py1-this.py0};};

  const b=new Panel('p7b',{persp:true});
  b.build=function(){
    const W=this.W,H=this.H;this.setupPersp({elev:30,yaw:-8,fov:28,dist:1.12,target:[0,92,0]});
    const L=-W/2+44,pw=W-62,slot=pw/28,bw=slot*.64,S=1.5;
    const floor=new THREE.Mesh(new THREE.BoxGeometry(W-8,3,70),new THREE.MeshStandardMaterial({color:T.label,transparent:true,opacity:T.dark?.07:.05,roughness:.2}));floor.position.set(0,-2,0);this.scene.add(floor);
    const wall=new THREE.Mesh(new THREE.PlaneGeometry(pw+8,150),new THREE.MeshBasicMaterial({color:T.label,transparent:true,opacity:T.dark?.04:.03,side:2,depthWrite:false}));wall.position.set(L+pw/2,75,-22);this.scene.add(wall);
    for(const v of [0,25,50,75,100]){const p=this.proj(L-6,v*S,-22);this.text(p.x,p.y,v,'l3','r');
      if(v>0){const gl=new THREE.Mesh(new THREE.PlaneGeometry(pw,.8),new THREE.MeshBasicMaterial({color:T.label,transparent:true,opacity:.1}));gl.position.set(L+pw/2,v*S,-21.5);this.scene.add(gl);}}
    const inst=new THREE.InstancedMesh(new THREE.BoxGeometry(1,1,1),this.std(0xffffff,{emissive:0x000000,emissiveIntensity:0}),28);const d=new THREE.Object3D();this.cols=[];
    TL.forEach((t,i)=>{const x=L+slot*i+slot/2;d.position.set(x,t.v*S/2,0);d.scale.set(bw,t.v*S,22);d.updateMatrix();inst.setMatrixAt(i,d.matrix);inst.setColorAt(i,t.v>=75?T.copper:mixc(T.accent,T.content,.25));
      const top=this.proj(x,t.v*S,0);this.cols.push({x:top.x,y:top.y,t});});
    this.scene.add(inst);
    const p7=TL.map((t,i)=>({x:L+slot*i+slot/2,y:t.a7*S})),p28=TL.map((t,i)=>({x:L+slot*i+slot/2,y:t.a28*S}));
    const r7=ribbonMesh(p7,{w:1.6,color:T.label,z:16,depth:true});this.scene.add(r7);
    const g7=ribbonMesh(p7,{w:4.5,color:T.label,op:.14,z:15.5,depth:true});this.scene.add(g7);
    for(let i=0;i<27;i++){if(i%2)continue;const s=ribbonMesh([p28[i],p28[i+1]],{w:1.8,color:T.copper,z:16,depth:true});this.scene.add(s);}
    [0,7,14,21,27].forEach(i=>{const p=this.proj(L+slot*i+slot/2,0,12);this.text(p.x,p.y+11,i===27?'today':sshort(dayAt(TL[i].back)),'l3','c');});
    this.text(this.proj(L-40,170,-22).x,this.proj(L-40,170,-22).y,`<b class="t17" style="font-weight:400">${TL[27].a7.toFixed(0)}</b> <i>7-day avg</i> &nbsp; <b class="t17" style="font-weight:400;color:var(--c-copper-text)">${TL[27].a28.toFixed(0)}</b> <i>28-day avg</i>`,'t13','l');
    // dial
    const dc=new THREE.Vector3(W/2-80,215,-20);const seg=(f0,f1,col,op)=>{const m=new THREE.Mesh(new THREE.TorusGeometry(36,5,12,40,(f1-f0)*Math.PI),this.std(col,{transparent:true,opacity:op,emissiveIntensity:T.dark?.35:.1}));m.position.copy(dc);m.rotation.z=Math.PI-f1*Math.PI;this.scene.add(m);};
    const F=v=>(v-.5)/1.3;seg(0,F(.8),T.label3,.55);seg(F(.8),F(1.3),T.accent,1);seg(F(1.3),1,T.copper,1);
    const ang=Math.PI-F(RATIO)*Math.PI;const nd=new THREE.Mesh(new THREE.BoxGeometry(30,2.6,2.6),new THREE.MeshBasicMaterial({color:T.label}));
    nd.position.set(dc.x+Math.cos(ang)*15,dc.y+Math.sin(ang)*15,dc.z+4);nd.rotation.z=ang;this.scene.add(nd);
    const hub=new THREE.Mesh(new THREE.SphereGeometry(4.5,16,12),new THREE.MeshBasicMaterial({color:T.label}));hub.position.set(dc.x,dc.y,dc.z+4);this.scene.add(hub);
    const q=this.proj(dc.x,dc.y-18,dc.z);this.text(q.x,q.y+4,`<b class="t13">Ratio ${RATIO.toFixed(2)}</b>`,'t13','c');this.text(q.x,q.y+19,`${ratioTxt} · sweet spot 0.8 to 1.3`,'l3','c');
    this.upd=()=>{if(this.key&&!REDUCED)this.key.position.x=-W*.3+Math.sin(performance.now()/2600)*W*.3;};
  };
  b.pick=function(x,y){let best=null,bd=1e9;this.cols.forEach(c=>{const d=Math.abs(c.x-x);if(d<bd){bd=d;best=c;}});
    if(!best||bd>14||y<30)return null;return{tip:tlTip(best.t),ax:best.x,ay:best.y,hx:best.x,hy0:best.y,hy1:best.y+4};};
}
