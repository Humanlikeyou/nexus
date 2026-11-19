/* ===== Panel 3: HR take ===== */
{
  const p3=new Panel('p3');
  const r=rng(21);const N=180;const hr=[];
  for(let i=0;i<N;i++){const t=i/N;let v=69+5*Math.sin(t*5.2)+7*Math.max(0,Math.sin(t*9-1.2))*(t>.45?1:0)+(r()-.5)*4.4;hr.push(Math.round(v));}
  for(let i=96;i<113;i++)hr[i]=null;
  const LO=62,HI=78,YMIN=52,YMAX=96;
  const NOW=new Date(2026,10,18,14,32,50);
  const tlab=i=>{const d=new Date(NOW.getTime()-(N-1-i)*5000);return `${String(d.getHours()).padStart(2,'0')}:${String(d.getMinutes()).padStart(2,'0')}:${String(d.getSeconds()).padStart(2,'0')}`;};
  p3.build=function(){
    const W=this.W,H=this.H,L=46,Rg=86,Tp=14,B=30,pw=W-L-Rg,ph=H-Tp-B;
    const X=i=>L+i/(N-1)*pw,Y=v=>Tp+(1-(v-YMIN)/(YMAX-YMIN))*ph;
    this.X=X;this.Y=Y;this.L=L;this.pw=pw;this.Tp=Tp;this.ph=ph;
    for(const v of [60,70,80,90]){this.hline(L,L+pw,Y(v),T.label,.08);this.text(L-9,Y(v),v,'','r');}
    this.text(L-9,Tp-4,'bpm','l3','r');
    const band=this.rect(L,Y(HI),pw,Y(LO)-Y(HI),T.accent,T.dark?.16:.12);
    this.hline(L,L+pw,Y(HI),T.accent,.45);this.hline(L,L+pw,Y(LO),T.accent,.45);
    this.text(L+pw+8,Y(HI),'Usual<br>62 to 78','l3','l');
    [['−15 min',0],['−10',N/3],['−5',N*2/3],['now',N-1]].forEach(([s,i])=>this.text(X(i),H-B+16,s,'l3',i===0?'l':i===N-1?'r':'c'));
    // segments
    const segs=[];let cur=[];hr.forEach((v,i)=>{if(v==null){if(cur.length)segs.push(cur);cur=[];}else cur.push(i);});if(cur.length)segs.push(cur);
    const colOf=v=>v>HI?T.copper:T.accent;
    for(const s of segs){
      const pts=s.map(i=>({x:X(i),y:Y(hr[i])}));const cols=s.map(i=>colOf(hr[i]).clone());
      this.area(pts,Tp+ph,{color:T.accent,a0:T.dark?.22:.16,a1:0});
      this.ribbon(pts,{w:4.2,cols:cols.map(c=>c.clone().multiplyScalar(1)),op:T.dark?.2:.14,blend:glowBlend()});
      this.ribbon(pts,{w:1.6,cols});
    }
    // gap bridge
    const g0=segs[0][segs[0].length-1],g1=segs[1][0];
    this.dashed(X(g0),Y(hr[g0]),X(g1),Y(hr[g1]),T.label3,.8,4,4,1.4);
    this.text((X(g0)+X(g1))/2,Y(hr[g0])+((hr[g0]>70)?-18:22),'No signal for 1 min 25 s','l3','c');
    // last point
    const li=N-1;this.lx=X(li);this.ly=Y(hr[li]);
    this.ring=this.add(new THREE.Mesh(new THREE.RingGeometry(.86,1,48),mat2(T.accent,.6)));this.ring.position.set(this.lx,this.ly,0);
    this.circle(this.lx,this.ly,4.6,T.accent,1);
    this.text(this.lx+12,this.ly-2,`<b class="t21" style="font-weight:300">${hr[li]}</b> <i>bpm now</i>`,'','l');
    // particles
    const NP=70;this.pm=pointsMesh(NP,2.6);this.add(this.pm);
    const valid=[];hr.forEach((v,i)=>{if(v!=null)valid.push(i);});
    this.par=Array.from({length:NP},()=>({i:valid[Math.floor(r()*valid.length)],a:r(),s:.12+r()*.18,dx:(r()-.5)*10}));
    this.upd=(t,dt)=>{
      const pos=this.pm.geometry.attributes.position,col=this.pm.geometry.attributes.color;
      this.par.forEach((q,k)=>{q.a+=dt*q.s*(REDUCED?0:1);if(q.a>1){q.a=0;q.i=valid[Math.floor(r()*valid.length)];}
        pos.setXYZ(k,X(q.i)+q.dx*q.a,Y(hr[q.i])-4-q.a*26,0);const c=hr[q.i]>HI?T.copper:T.light;col.setXYZW(k,c.r,c.g,c.b,(1-q.a)*(T.dark?.7:.55)*Math.min(1,q.a*8));});
      pos.needsUpdate=col.needsUpdate=true;
      const k=REDUCED?.5:(t%1.8)/1.8;this.ring.scale.set(5+k*20,5+k*20,1);this.ring.material.opacity=.7*(1-k);
    };
  };
  p3.pick=function(x,y){
    if(x<this.L||x>this.L+this.pw)return null;
    let i=Math.round((x-this.L)/this.pw*(N-1));if(hr[i]==null){let d=1;while(i-d>=0||i+d<N){if(hr[i-d]!=null){i-=d;break;}if(hr[i+d]!=null){i+=d;break;}d++;}}
    const v=hr[i],st=v>HI?'above usual':v<LO?'below usual':'inside usual';
    return{tip:`<b>${v} bpm</b> · ${tlab(i)}<small>Usual ${LO} to ${HI} bpm · ${st}</small>`,ax:this.X(i),ay:this.Y(v),hx:this.X(i),hy0:this.Tp,hy1:this.Tp+this.ph,px:this.X(i),py:this.Y(v),pc:v>HI?T.copper:T.accent};
  };
}
