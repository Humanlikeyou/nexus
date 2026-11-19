/* ===== Panel 4: Readiness ===== */
const DRV=[
  {n:'Heart-rate variability',v:'68 ms',u:'54 to 67 ms',z:1.3,w:'above usual'},
  {n:'Resting heart rate',v:'52 bpm',u:'51 to 57 bpm',z:.9,w:'lower than usual'},
  {n:'Sleep',v:'7 h 42 min',u:'6 h 50 to 7 h 40',z:1.1,w:'above usual'},
  {n:'Respiratory rate',v:'14.6 /min',u:'14.1 to 14.9',z:.2,w:'within usual'},
  {n:'Skin temperature',v:'+0.1 °C',u:'−0.2 to +0.3',z:-.1,w:'within usual'},
  {n:"Yesterday's strain",v:'71',u:'38 to 62',z:-1.8,w:'higher than usual'}];
const READY=78;
function readinessCommon(p,A){
  const W=p.W,H=p.H,tx0=190,tx1=W-160,half=(tx1-tx0)/2,cxm=(tx0+tx1)/2;
  const y0=A?138:0,pitch=A?(H-y0-14)/6:0;
  return{W,H,tx0,tx1,half,cxm,y0,pitch};
}
function readyHeader(p,x,y){
  p.text(x+90,y-14,'Ready','t21 k','l');
  p.text(x+90,y+10,'Above your usual · usual 64 to 76','l3 t11','l');
}
{
  const a=new Panel('p4a');
  a.build=function(){
    const c=readinessCommon(this,true);this.c=c;
    // score arc
    const cx=48,cy=58,r=34,full=Math.PI*1.5,start=Math.PI*.75;
    const arc=(f0,f1,w,col,op)=>{const pts=[];const n=48;for(let i=0;i<=n;i++){const an=start+(f0+(f1-f0)*i/n)*full;pts.push({x:cx+Math.cos(an)*r,y:cy+Math.sin(an)*r});}return this.ribbon(pts,{w,color:col,op});};
    arc(0,1,2.2,T.label,.12);arc(.64,.76,5,T.accent,.18);
    this.arcFill=arc(0,READY/100,2.6,T.accent,1);arc(0,READY/100,6,T.accent,T.dark?.22:.12);
    this.text(cx,cy,'78','t27 big','c');
    this.text(cx+52,cy-12,'Ready','t21 k','l');
    this.text(cx+52,cy+11,'Above your usual · usual 64 to 76','l3','l');
    this.text(c.tx0,c.y0-24,'Each bar shows how far today is from your usual','l3','l');
    this.text(c.cxm,c.y0-8,'<span style="color:var(--c-label3)">worse ← usual → better</span>','','c');
    this.fills=[];
    DRV.forEach((d,i)=>{
      const y=c.y0+i*c.pitch+c.pitch/2;
      if(i)this.hline(0,c.W,y-c.pitch/2,T.label,.06);
      this.text(4,y-6,d.n,'t13 k','l');this.text(4,y+9,d.w,d.z>.5?'up':d.z<-.5?'dn':'l3','l');
      this.rect(c.cxm-c.half*.4,y-5,c.half*.8,10,T.accent,T.dark?.14:.1);
      this.rect(c.tx0,y-1,c.tx1-c.tx0,2,T.label,.12);
      this.rect(c.cxm-.5,y-6,1,12,T.label3,1);
      const col=d.z>=.4?T.accent:d.z<=-.4?T.copper:T.label2;
      const reach=Math.min(Math.abs(d.z),2.5)/2.5*c.half;
      const f=this.rect(0,0,1,2.6,col,1);const g=this.rect(0,0,1,8,col,T.dark?.22:.14);const e=this.circle(0,0,3.6,col,1);
      this.fills.push({f,g,e,reach,side:d.z>=0?1:-1,y});
      this.text(c.tx1+14,y-6,d.v,'t13 k','l');this.text(c.tx1+14,y+9,'usual '+d.u,'l3','l');
    });
    this.upd=(t)=>{const k=REDUCED?1:ease(t/.9);this.fills.forEach(o=>{const w=Math.max(1,o.reach*k);const x=o.side>0?c.cxm+w/2:c.cxm-w/2;
      o.f.scale.x=w;o.f.position.set(x,o.y,0);o.g.scale.x=w;o.g.position.set(x,o.y,0);o.e.position.set(c.cxm+o.side*w,o.y,0);});};
  };
  a.pick=function(x,y){
    const c=this.c;const i=Math.floor((y-c.y0)/c.pitch);if(i<0||i>5)return null;const d=DRV[i];
    return{tip:`<b>${d.n}</b> ${d.v}<small>Usual ${d.u} · ${d.z>=0?'+':'−'}${Math.abs(d.z).toFixed(1)} σ, ${d.w}</small>`,ax:x,ay:c.y0+i*c.pitch,bx:0,by:c.y0+i*c.pitch,bw:c.W,bh:c.pitch};
  };

  const b=new Panel('p4b',{persp:true});
  b.build=function(){
    const W=this.W,H=this.H;this.setupPersp({elev:60,fov:28,dist:1.12,target:[0,0,18]});
    const tx0=-W/2+190,tx1=W/2-150,half=(tx1-tx0)/2,cxm=(tx0+tx1)/2,pz=44,z0=-pz*2.5+30;
    const floor=new THREE.Mesh(new THREE.BoxGeometry(W-8,3,pz*6+24),new THREE.MeshStandardMaterial({color:T.label,transparent:true,opacity:T.dark?.07:.05,roughness:.2}));floor.position.set(0,-2,z0+pz*2.5-pz*.5+0);this.scene.add(floor);
    const wall=new THREE.Mesh(new THREE.BoxGeometry(1.5,30,pz*6),new THREE.MeshBasicMaterial({color:T.label3,transparent:true,opacity:.35}));wall.position.set(cxm,15,z0+pz*2.5-pz*.5);this.scene.add(wall);
    this.rows=[];this.bars=[];
    DRV.forEach((d,i)=>{
      const z=z0+i*pz;
      const zone=new THREE.Mesh(new THREE.BoxGeometry(half*.8,2,18),new THREE.MeshBasicMaterial({color:T.accent,transparent:true,opacity:T.dark?.22:.16}));zone.position.set(cxm,1,z);this.scene.add(zone);
      const trk=new THREE.Mesh(new THREE.BoxGeometry(tx1-tx0,1,3),new THREE.MeshBasicMaterial({color:T.label,transparent:true,opacity:.2}));trk.position.set(cxm,.5,z);this.scene.add(trk);
      const col=d.z>=.4?T.accent:d.z<=-.4?T.copper:mixc(T.label2,T.content,.2);
      const reach=Math.min(Math.abs(d.z),2.5)/2.5*half;
      const box=new THREE.Mesh(new THREE.BoxGeometry(1,13,13),this.std(col,{emissiveIntensity:T.dark?.3:.12}));box.position.set(cxm,7,z);this.scene.add(box);
      this.bars.push({box,reach,side:d.z>=0?1:-1});
      const l=this.proj(-W/2+4,0,z),v=this.proj(tx1+14,0,z);
      this.text(4,l.y-6,d.n,'t13 k','l');this.text(4,l.y+9,d.w,d.z>.5?'up':d.z<-.5?'dn':'l3','l');
      this.text(W/2+tx1+14,v.y-6,d.v,'t13 k','l');this.text(W/2+tx1+14,v.y+9,'usual '+d.u,'l3','l');
      this.rows.push({y:l.y});
    });
    // score torus
    const tc=new THREE.Mesh(new THREE.TorusGeometry(30,4,16,64,Math.PI*1.5*READY/100),this.std(T.accent,{emissiveIntensity:T.dark?.5:.2}));
    tc.rotation.x=-Math.PI/2;tc.rotation.z=-Math.PI*.75-Math.PI*1.5*READY/100+Math.PI*1.5*.0+Math.PI*.0;tc.position.set(-W/2+54,6,z0-pz*0+6-70);
    const tb=new THREE.Mesh(new THREE.TorusGeometry(30,1.4,8,64,Math.PI*1.5),new THREE.MeshBasicMaterial({color:T.label,transparent:true,opacity:.14}));tb.rotation.copy(tc.rotation);tb.rotation.z=-Math.PI*.75-Math.PI*1.5;tb.position.copy(tc.position);
    tc.rotation.z=-Math.PI*.75-Math.PI*1.5;
    this.scene.add(tb);this.scene.add(tc);
    const sc=this.proj(tc.position.x,6,tc.position.z);
    this.text(sc.x,sc.y,'78','t27 big','c');this.text(sc.x+52,sc.y-8,'Ready','t21 k','l');this.text(sc.x+52,sc.y+14,'Above your usual · usual 64 to 76','l3','l');
    this.upd=(t)=>{const k=REDUCED?1:ease(t/.9);this.bars.forEach(o=>{const w=Math.max(1,o.reach*k);o.box.scale.x=w;o.box.position.x=cxm+o.side*w/2;});
      if(this.key&&!REDUCED)this.key.position.x=-W*.4+Math.sin(t*.5)*W*.35;};
  };
  b.pick=function(x,y){
    let bi=-1,bd=1e9;this.rows.forEach((r,i)=>{const d=Math.abs(r.y-y);if(d<bd){bd=d;bi=i;}});
    if(bd>26)return null;const d=DRV[bi];
    return{tip:`<b>${d.n}</b> ${d.v}<small>Usual ${d.u} · ${d.z>=0?'+':'−'}${Math.abs(d.z).toFixed(1)} σ, ${d.w}</small>`,ax:x,ay:this.rows[bi].y};
  };
}
