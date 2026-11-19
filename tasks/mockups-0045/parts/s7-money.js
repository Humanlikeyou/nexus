/* ===== Panel 8: Money ===== */
const MONTHS=['Dec','Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov'];
const INC=[5640,5640,5820,5640,6120,5640,5640,5900,5640,5640,5780,5820];
const EXP=[4380,4710,4120,4560,4880,4310,4950,4230,4470,4690,4260,4540];
const SAV=(()=>{let s=9200;return INC.map((v,i)=>{s+=v-EXP[i];return s;});})();
const TARGET=SAV.map((_,i)=>9200+(30000-9200)*(i+1)/12);
const BAL=24860;
const CATS=[['Housing',1450],['Food and groceries',830],['Fun and travel',500],['Transport',360],['Health',320],['Subscriptions',270],['Other',810]];
const CATTOT=CATS.reduce((s,c)=>s+c[1],0);
const catCol=i=>[T.accent,mixc(T.accent,T.content,.35),T.copper,mixc(T.copper,T.content,.4),mixc(T.accent,T.label,.35),mixc(T.label3,T.content,.2),mixc(T.label2,T.content,.45)][i];
const css=c=>'#'+c.getHexString();
function moneyDom(){
  const mk=(n,v,s,z)=>`<div class="chip"><span>${n}</span><strong>${v}</strong>${z==null?'':dbarHtml(z)}<em>${s}</em></div>`;
  $('#mstats').innerHTML=mk('Balance',usd(BAL),'about '+jpy(BAL*RATE),null)+mk('Income this month',usd(INC[11]),jpy(INC[11]*RATE),.4)+mk('Spent this month',usd(EXP[11]),jpy(EXP[11]*RATE)+' · within usual',-.3)+mk('Savings rate',Math.round((1-EXP[11]/INC[11])*100)+'%','Target 20% · above',.9);
  $('#catlist').innerHTML=CATS.map((c,i)=>`<div><s style="background:${css(catCol(i))}"></s><span>${c[0]}</span><em>${usd(c[1])} · ${Math.round(c[1]/CATTOT*100)}%</em></div>`).join('')+`<div style="margin-top:4px;color:var(--c-label2)"><span></span><span>Total</span><em>${usd(CATTOT)} · ${jpy(CATTOT*RATE)}</em></div>`;
}
document.addEventListener('themechange',moneyDom);
{
  const m1=new Panel('m1');
  m1.build=function(){
    const W=this.W,H=this.H,L=48,R=6,pw=W-L-R,py0=24,py1=H-26,ph=py1-py0,Y=v=>py1-v/7000*ph;
    this.L=L;this.pw=pw;this.slot=pw/12;this.py0=py0;this.py1=py1;this.Y=Y;
    for(const v of [0,2000,4000,6000]){this.hline(L,L+pw,Y(v),T.label,v?.06:.14);this.text(L-6,Y(v),v?`$${v/1000}k`:'$0','l3','r');}
    const s=pw/12,bw=Math.min(18,s*.33);
    const list=[];MONTHS.forEach((mm,i)=>{const cx=L+s*i+s/2;list.push({x:cx-bw-1,y:Y(INC[i]),w:bw,h:py1-Y(INC[i]),c:T.accent},{x:cx+1,y:Y(EXP[i]),w:bw,h:py1-Y(EXP[i]),c:T.copper});this.text(cx,py1+13,mm,'l3','c');});
    this.rects(list);
    this.text(L,8,'<span style="color:var(--c-accent2)">■</span> Income &nbsp; <span style="color:var(--c-copper-text)">■</span> Expenses','','l');
  };
  m1.pick=function(x,y){const i=Math.floor((x-this.L)/this.slot);if(i<0||i>11)return null;
    return{tip:`<b>${MONTHS[i]}</b><br>Income ${usd(INC[i])}<br>Expenses ${usd(EXP[i])}<small>Saved ${usd(INC[i]-EXP[i])} · ${jpy((INC[i]-EXP[i])*RATE)}</small>`,ax:this.L+this.slot*(i+.5),ay:this.Y(Math.max(INC[i],EXP[i])),bx:this.L+this.slot*i,by:this.py0,bw:this.slot,bh:this.py1-this.py0};};

  const m2=new Panel('m2');
  m2.build=function(){
    const W=this.W,H=this.H,L=48,R=10,pw=W-L-R,py0=24,py1=H-26,ph=py1-py0,Y=v=>py1-(v-8000)/(32000-8000)*ph,X=i=>L+i/11*pw;
    this.L=L;this.pw=pw;this.py0=py0;this.py1=py1;this.Y=Y;this.X=X;
    for(const v of [10000,20000,30000]){this.hline(L,L+pw,Y(v),T.label,.07);this.text(L-6,Y(v),`$${v/1000}k`,'l3','r');}
    const pts=SAV.map((v,i)=>({x:X(i),y:Y(v)}));
    this.area(pts,py1,{color:T.accent,a0:T.dark?.3:.2,a1:0});
    for(let i=0;i<11;i++)this.dashed(X(i),Y(TARGET[i]),X(i+1),Y(TARGET[i+1]),T.copper,1,5,4,1.6);
    this.ribbon(pts,{w:4.5,color:T.accent,op:T.dark?.2:.14,blend:glowBlend()});this.ribbon(pts,{w:1.7,color:T.accent});
    this.circle(pts[11].x,pts[11].y,4,T.accent,1);
    MONTHS.forEach((mm,i)=>{if(i%2===1||i===11)this.text(X(i),py1+13,mm,'l3','c');});
    this.text(X(11),Y(TARGET[11])-10,'Target $30,000','dn','r');
    this.text(L+4,py0+2,`<b class="t17" style="font-weight:400">${usd(SAV[11])}</b> <i>saved · ${jpy(SAV[11]*RATE)}</i>`,'t13','l');
    this.text(X(11)-6,pts[11].y+30,SAV[11]>=TARGET[11]?'On track':'Behind target by '+usd(TARGET[11]-SAV[11]),SAV[11]>=TARGET[11]?'up':'dn','r');
  };
  m2.pick=function(x){if(x<this.L-4)return null;const i=clamp(Math.round((x-this.L)/this.pw*11),0,11);
    return{tip:`<b>${MONTHS[i]}</b> saved ${usd(SAV[i])}<small>Target ${usd(Math.round(TARGET[i]))} · ${SAV[i]>=TARGET[i]?'ahead by '+usd(Math.round(SAV[i]-TARGET[i])):'behind by '+usd(Math.round(TARGET[i]-SAV[i]))}</small>`,ax:this.X(i),ay:this.Y(SAV[i]),hx:this.X(i),hy0:this.py0,hy1:this.py1,px:this.X(i),py:this.Y(SAV[i])};};

  const m3=new Panel('m3',{persp:true});
  m3.build=function(){
    const S=Math.min(this.W*.36,this.H*.4);this.setupPersp({elev:50,fov:28,dist:1.0,target:[0,S*.1,0]});
    const g=new THREE.Group();g.scale.setScalar(S);this.scene.add(g);this.sl=[];
    let a=Math.PI/2;
    CATS.forEach((c,i)=>{const sw=c[1]/CATTOT*Math.PI*2;const a1=a-.012,a0=a-sw+.012;const m=new THREE.Mesh(sectorGeo(a0,a1,.58,1),this.std(catCol(i)));m.rotation.x=-Math.PI/2;m.scale.z=.12+c[1]/1450*.18;m.userData.i=i;g.add(m);this.sl.push(m);a-=sw;});
    const c=this.proj(0,0,0);this.text(c.x,c.y-8,`<b class="t21" style="font-weight:300">${usd(CATTOT)}</b>`,'t21','c');this.text(c.x,c.y+14,'spent in Nov','l3','c');
  };
  m3.pick=function(x,y){const rc=new THREE.Raycaster();rc.setFromCamera(new THREE.Vector2(x/this.W*2-1,-(y/this.H*2-1)),this.cam);
    const h=rc.intersectObjects(this.sl,false)[0];if(!h)return null;const i=h.object.userData.i,c=CATS[i];
    return{tip:`<b>${c[0]}</b> ${usd(c[1])}<small>${Math.round(c[1]/CATTOT*100)}% · ${jpy(c[1]*RATE)}</small>`,ax:x,ay:y,idx:i};};
  m3.onHover=function(h){this.sl?.forEach((m,i)=>{m.material.emissiveIntensity=h&&h.idx===i?.6:(T.dark?.14:.06);});};
}

/* ---- go ---- */
start();
moneyDom();
