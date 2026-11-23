/* ===== Panel 1: Neuro last session ===== */
const BANDS=[
  {k:'Delta',hz:'1 to 4 Hz',p:18,u:[14,22]},{k:'Theta',hz:'4 to 8 Hz',p:14,u:[11,17]},
  {k:'Alpha',hz:'8 to 13 Hz',p:31,u:[22,30]},{k:'Beta',hz:'13 to 30 Hz',p:24,u:[20,29]},{k:'Gamma',hz:'30 to 45 Hz',p:13,u:[9,15]}];
const bandColor=i=>mixc(T.copper,T.accent,i/4).lerp(T.dark?new THREE.Color(0xffffff):new THREE.Color(0x000000),i===4?.12:0);
{
  const p1=new Panel('p1',{persp:true});
  p1.build=function(){
    const S=Math.min(this.W*.3,this.H*.34);
    this.setupPersp({elev:60,fov:28,dist:1.05,target:[0,S*.1,0]});
    const g=new THREE.Group();g.scale.setScalar(S);this.scene.add(g);
    const hf=v=>v/40*.62;
    const floor=new THREE.Mesh(new THREE.RingGeometry(.42,1.12,96),new THREE.MeshBasicMaterial({color:T.label,transparent:true,opacity:.06,side:2}));floor.rotation.x=-Math.PI/2;g.add(floor);
    for(const r of [.5,1.0]){const l=new THREE.Mesh(new THREE.RingGeometry(r-.004,r+.004,96),new THREE.MeshBasicMaterial({color:T.label,transparent:true,opacity:.28,side:2}));l.rotation.x=-Math.PI/2;l.position.y=.002;g.add(l);}
    this.bars=[];
    BANDS.forEach((b,i)=>{
      const a1=Math.PI/2-i*Math.PI*2/5-.05,a0=Math.PI/2-(i+1)*Math.PI*2/5+.05,mid=(a0+a1)/2;
      const geo=sectorGeo(a0,a1,.5,1.0);
      const col=bandColor(i);
      const m=new THREE.Mesh(geo,this.std(col));m.rotation.x=-Math.PI/2;m.scale.z=hf(b.p);m.userData.i=i;g.add(m);this.bars.push(m);
      const gh=new THREE.Mesh(geo,new THREE.MeshBasicMaterial({color:T.label,transparent:true,opacity:T.dark?.14:.12,depthWrite:false}));
      gh.rotation.x=-Math.PI/2;gh.position.y=hf(b.u[0]);gh.scale.z=hf(b.u[1])-hf(b.u[0]);g.add(gh);
      const ed=new THREE.LineSegments(new THREE.EdgesGeometry(geo,30),new THREE.LineBasicMaterial({color:T.label,transparent:true,opacity:.5}));
      ed.rotation.x=-Math.PI/2;ed.position.y=gh.position.y;ed.scale.z=gh.scale.z;g.add(ed);
      const hi=b.p>b.u[1],lo=b.p<b.u[0];
      const q=this.proj(.78*Math.cos(mid)*S,(hf(b.p)+.08)*S,-.78*Math.sin(mid)*S);
      this.text(q.x,q.y,`<b>${b.k}</b><br><i>${b.p}%</i>${hi?' <span style="color:var(--c-accent2)">▲</span>':lo?' <span style="color:var(--c-copper-text)">▼</span>':''}`,'two t13 pill','bc');
      b.mid=mid;b.S=S;
    });
    const c=this.proj(0,0,0);
    this.text(c.x,c.y-14,'Wed 18 Nov','l3','c');
    this.text(c.x,c.y+8,'24 min','t21 k','c');
    this.text(this.W-14,this.H-14,'<span style="color:var(--c-label2)">Bar height = share of band power, ghost slab = usual range</span>','','br');
    this.hs=S;
  };
  p1.pick=function(x,y){
    const rc=new THREE.Raycaster();rc.setFromCamera(new THREE.Vector2(x/this.W*2-1,-(y/this.H*2-1)),this.cam);
    const hit=rc.intersectObjects(this.bars,false)[0];if(!hit)return null;
    const i=hit.object.userData.i,b=BANDS[i];const S=this.hs;
    const st=b.p>b.u[1]?'above usual':b.p<b.u[0]?'below usual':'within usual';
    return{tip:`<b>${b.k}</b> ${b.hz}<br>${b.p}% of session power<small>Usual ${b.u[0]} to ${b.u[1]}% · ${st}</small>`,ax:x,ay:y,idx:i};
  };
  p1.onHover=function(h){this.bars?.forEach((m,i)=>{m.material.emissiveIntensity=h&&h.idx===i?.55:(T.dark?.14:.06);});};
  const chips=$('#p1chips');
  const mk=(n,v,u,z,txt)=>`<div class="chip"><span>${n}</span><strong>${v}<small>${u}</small></strong>${dbarHtml(z)}<em>${txt}</em></div>`;
  chips.innerHTML=mk('Focus',68,'/100',1.3,'Usual 55 to 66 · above your usual')+mk('Calm',71,'/100',0.2,'Usual 60 to 72 · within your usual')+mk('Duration',24,'min',-0.6,'Usual 20 to 30 min');
}

/* ===== Panel 2: Neuro now ===== */
{
  const p2=new Panel('p2');
  const live={v:[16,13,28,26,17],tgt:[16,13,28,26,17],on:1,tgtOn:1,ph:[0,1.3,2.1,3.4,4.2]};
  const R=rng(7);
  const KF=[2,3,5,8,13];
  p2.build=function(){
    const H=this.H,W=this.W;const R0=Math.min(H/2-22,(W-210)/2-14);const cx=R0+26,cy=H/2;this.R0=R0;this.cx=cx;this.cy=cy;
    this.rings=[];const sp=R0*.13;
    BANDS.forEach((b,i)=>{
      const r=R0*.44+i*sp+sp*.3;const col=bandColor(i);
      const ampHi=b.u[1]/40*sp*.62*1.25;
      const env=this.add(ribbonMesh(ringPts(cx,cy,r,120),{w:ampHi,color:col,op:T.dark?.12:.14}));
      const core=this.add(ribbonMesh(ringPts(cx,cy,r,200),{w:1.4,color:col,op:1}));
      const glow=this.add(ribbonMesh(ringPts(cx,cy,r,200),{w:4.5,color:col,op:T.dark?.22:.14,blend:glowBlend()}));
      this.rings.push({r,core,glow,env,sp,ampHi});
      this.text(cx+R0+34,cy-44+i*22,`<b>${b.k}</b>`,'t13','l');
    });
    this.vals=BANDS.map((b,i)=>this.text(this.W-12,cy-44+i*22,'','t13 k','r'));
    this.text(cx+R0+34,cy+82,'Live share of power, usual in grey','l3','l');
    this.center=this.text(cx,cy-6,'','two','c');
    this.note=this.text(cx,cy+R0*.44-6,'','l3','c');
    this.cleanup=null;
    this.upd=(t,dt)=>{
      live.on=lerp(live.on,live.tgtOn,Math.min(1,dt*4));
      if(REDUCED)live.on=live.tgtOn;
      if(Math.floor(t*10)!==this._tick){this._tick=Math.floor(t*10);
        for(let i=0;i<5;i++){const b=BANDS[i];live.tgt[i]=clamp(live.tgt[i]+(R()-.5)*1.6+(b.p-live.tgt[i])*.03,b.u[0]-3,b.u[1]+4);}
      }
      for(let i=0;i<5;i++)live.v[i]=lerp(live.v[i],live.tgt[i],Math.min(1,dt*6));
      this.rings.forEach((rg,i)=>{
        const amp=live.v[i]/40*rg.sp*.62*live.on;
        const upd=(mesh,w)=>{
          const pts=ringPts(cx,cy,rg.r,200,a=>amp*Math.sin(KF[i]*a+t*(.8+i*.25)+live.ph[i])*(1+.15*Math.sin(a*2+t)));
          const nm=ribbonMesh(pts,{w});mesh.geometry.dispose();mesh.geometry=nm.geometry;nm.material.dispose();
        };
        upd(rg.core,1.4);upd(rg.glow,4.5);
        rg.core.material.opacity=lerp(.25,1,live.on);rg.glow.material.opacity=(T.dark?.22:.14)*live.on;rg.env.material.opacity=(T.dark?.12:.14)*(.3+.7*live.on);
      });
      if(!this._lt||t-this._lt>.2){this._lt=t;
        this.vals.forEach((s,i)=>{s.innerHTML=live.on>.5?`${live.v[i].toFixed(0)}% <i>usual ${BANDS[i].u[0]} to ${BANDS[i].u[1]}</i>`:'<i>no data</i>';});
        const lead=live.v.indexOf(Math.max(...live.v));
        const calm=Math.round(live.v[2]/(live.v[2]+live.v[3])*130);
        this.center.innerHTML=live.on>.5?`<span class="l3">Leading</span><br><b class="t21" style="font-weight:300">${BANDS[lead].k}</b><br><span class="l3">Calm ${calm}</span>`:`<span class="l3">Muse</span><br><b class="t17" style="font-weight:400">Not connected</b>`;
        this.note.innerHTML=live.on>.5?'':'Shows while Muse is connected';
      }
    };
  };
  p2.pick=function(x,y){
    const d=Math.hypot(x-this.cx,y-this.cy);if(d>this.R0*1.1||d<this.R0*.4||live.on<.5)return null;
    let best=0,bd=1e9;this.rings.forEach((r,i)=>{const e=Math.abs(r.r-d);if(e<bd){bd=e;best=i;}});
    const b=BANDS[best],v=live.v[best];
    return{tip:`<b>${b.k}</b> ${b.hz}<br>${v.toFixed(1)}% now<small>Usual ${b.u[0]} to ${b.u[1]}%</small>`,ax:x,ay:y};
  };
  seg('', '#p2seg',v=>{live.tgtOn=v;});
}
