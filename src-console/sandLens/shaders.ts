import { LUT_W } from "./bessel";

/**
 * design-1118b sand shaders (ported from the Stage A mock, then re-looked per Taisun's FIGURA reference):
 * the same Bessel/Chladni physics; dense, heaped, matte cream grains in normal alpha blending (no glow),
 * per-grain diffuse from the canopy light with a darker underside, a soft contact shadow from a blurred
 * density map, and an organic uneven outer edge (per-session seed) that leaves the core physics untouched.
 */
export const COMMON = /* glsl */ `
uniform float uDt, uTime, uFrame, uState, uWakeT, uLoud, uMic, uBreathP, uCalm, uSnap, uKick, uWave;
uniform vec4 uCard; uniform float uBottom;
uniform sampler2D uLut; uniform vec4 uRow, uN, uA;
uvec3 pcg3d(uvec3 v){ v = v*1664525u + 1013904223u; v.x += v.y*v.z; v.y += v.z*v.x; v.z += v.x*v.y; v ^= v>>16u; v.x += v.y*v.z; v.y += v.z*v.x; v.z += v.x*v.y; return v; }
vec3 rnd3(uvec3 v){ return vec3(pcg3d(v)) * (1.0/4294967296.0); }
vec4 shash(){ uvec2 c=uvec2(gl_FragCoord.xy); vec3 a=rnd3(uvec3(c,17u)); vec3 b=rnd3(uvec3(c,91u)); return vec4(a,b.x); }
vec4 shash2(){ uvec2 c=uvec2(gl_FragCoord.xy); vec3 a=rnd3(uvec3(c,233u)); vec3 b=rnd3(uvec3(c,377u)); return vec4(a,b.y); }
float radial(float row, float r){ float x=clamp(r,0.,1.)*${(LUT_W - 1).toFixed(1)}; int i=int(floor(x)); int j=min(i+1,${LUT_W - 1}); float f=x-float(i);
  int ro=int(row+0.5); return mix(texelFetch(uLut, ivec2(i,ro),0).r, texelFetch(uLut, ivec2(j,ro),0).r, f); }
float field(vec2 p){ float r=length(p); float th=atan(p.y,p.x); float w=0.;
  for(int i=0;i<4;i++){ if(abs(uA[i])>1e-4) w+=uA[i]*radial(uRow[i],r)*cos(uN[i]*th); } return w; }
float homeAng(vec4 h){ return 6.2831853*h.y + (2.2+1.6*h.x)*2.4; }
vec2 homePos(vec4 h){ float r=0.965*sqrt(h.x); float a=homeAng(h); return r*vec2(cos(a),sin(a)); }
vec3 heapTarget(vec4 h, vec4 h2, out float lost){
  lost = step(h2.x, 0.02);
  float tri=(h.x+h.y+h.z+h.w)*0.25; tri=clamp((tri-0.5)*1.7+0.5,0.,1.);
  if(lost>0.5){ return vec3(mix(uCard.x-0.2,uCard.y+0.2,h.w), uBottom-0.5, 0.); }
  float x=mix(uCard.x+0.06,uCard.y-0.06,tri);
  float prof=(0.35+0.65*pow(1.0-abs(2.0*tri-1.0),1.2))*(0.6+0.4*sin(tri*29.0+1.3)*sin(tri*11.0+0.4));
  return vec3(x, uCard.z + uCard.w*prof*pow(h2.y,0.9), 0.);
}
vec2 nodal(vec2 p){ const float e=0.002;
  for(int i=0;i<10;i++){ float w=field(p); vec2 g=vec2(field(p+vec2(e,0.))-field(p-vec2(e,0.)), field(p+vec2(0.,e))-field(p-vec2(0.,e)))/(2.0*e);
    vec2 s=w*g/max(dot(g,g),1e-5); float sl=length(s); if(sl>0.05) s*=0.05/sl; p-=s; float r=length(p); if(r>0.995) p*=0.995/r; }
  if(abs(field(p))>0.02) p=normalize(p+1e-5)*0.995; return p; }
`;

export const VEL = COMMON + /* glsl */ `
void main(){
  vec2 uv=gl_FragCoord.xy/resolution.xy;
  vec3 p=texture2D(texturePosition,uv).xyz; vec3 v=texture2D(textureVelocity,uv).xyz;
  if(uCalm>0.5){ gl_FragColor=vec4(0.); return; }
  vec4 h=shash(), h2=shash2(); float dt=uDt; int st=int(uState+0.5);
  vec3 rn=rnd3(uvec3(uvec2(gl_FragCoord.xy), uint(uFrame)));
  vec3 rn2=rnd3(uvec3(uvec2(gl_FragCoord.xy)+uvec2(7u,13u), uint(uFrame)+101u));
  if(st==0){ // VOICE OFF: sand drifts down and settles on the chat card; a few grains fall past it
    float lost; vec3 tg=heapTarget(h,h2,lost);
    if(p.y<=tg.y+0.004){ v=vec3((tg.x-p.x)*2.0,0.,-p.z*3.0); }
    else{ v.y-=1.6*dt; v.y=max(v.y,-0.75-0.25*h.z); v.x+=((tg.x-p.x)*1.4-v.x*1.3)*dt+sin(uTime*0.6+h.z*30.)*0.12*dt; v.z+=(-p.z*4.0-v.z*2.5)*dt; }
  } else if(st==1){ // WAKE: lift off the card, swirl up, flatten into an even layer (~2 s)
    float t=uWakeT; float rise=smoothstep(0.0,0.6,t); float fl=smoothstep(0.5,0.95,t);
    float a=homeAng(h)+(2.2+1.6*h.x)*4.0*(min(t,0.6)-0.6);
    vec2 c=vec2(0., mix(uCard.z, 0., rise));
    float rad=mix(1.0,0.35+0.6*sqrt(h.x),rise)*(1.0+0.2*sin(3.1416*rise));
    vec3 vort=vec3(c+rad*vec2(cos(a),sin(a)), (0.1+0.9*h.z)*0.9*sin(3.1416*clamp(t/0.75,0.,1.)));
    vec3 tg=mix(vort, vec3(homePos(h),0.), fl);
    v+=((tg-p)*mix(9.0,14.0,fl)-v*mix(3.2,6.5,fl))*dt;
  } else if(st==2){ // LISTENING: an even layer, softly breathing; light reaction to the mic level
    vec2 hp=homePos(h); float br=sin(uTime*6.2831853/uBreathP + length(hp)*2.2);
    vec3 tg=vec3(hp*(1.0+0.012*br), 0.025*br*(0.5+h.z));
    v+=((tg-p)*7.0-v*5.0)*dt + (rn-0.5)*vec3(1.,1.,0.4)*uMic*1.2*dt*20.0;
  } else { // SPEAKING: Chladni physics. Grains hop where |w| is large and come to rest on the nodal lines.
    float r=length(p.xy);
    if(r>1.03 || p.z>0.35){ v+=((vec3(homePos(h),0.)-p)*8.0-v*4.0)*dt; }
    else if(p.z<=0.0008){
      float aw=abs(field(p.xy)); float L=uLoud;
      if(rn.x < clamp(aw*L*2.6,0.,1.)){ float an=rn.y*6.2831853; float s=aw*L*(0.25+0.75*rn.z)*1.8; v=vec3(cos(an)*s, sin(an)*s, aw*L*(0.8+0.8*rn2.x)*2.0); }
      else { v=vec3(0.); }
    } else { v.z-=40.0*dt; v.xy*=(1.0-1.2*dt); if(r>0.985 && dot(v.xy,p.xy)>0.) v.xy=reflect(v.xy,p.xy/r)*0.5; }
  }
  // micro-moments: a soft kick (task done) and a ripple ring (notification) make grains hop, then settle again
  if(st!=1){
    float r=length(p.xy);
    // task done: one soft "exhale" (grains hop up and outward, then the layer gathers back)
    if(uKick>0.97 && p.z<=0.06 && rn2.y < 0.7){ float an=rn2.z*6.2831853; v+=vec3(p.xy*1.6+vec2(cos(an),sin(an))*0.2, 0.9+0.8*rn.z)*uKick + vec3(0.,(st==0?0.3:0.0),0.); }
    // notification: a single ripple ring travels out from the centre
    if(uWave>0.0 && uWave<1.6 && abs(r-uWave)<0.06 && rn2.y<0.9){ v+=vec3(p.xy/max(r,1e-3)*0.09, 0.12+0.08*rn.z); }
  }
  gl_FragColor=vec4(v,0.);
}`;

export const POS = COMMON + /* glsl */ `
void main(){
  vec2 uv=gl_FragCoord.xy/resolution.xy;
  vec3 p=texture2D(texturePosition,uv).xyz; vec3 v=texture2D(textureVelocity,uv).xyz;
  vec4 h=shash(), h2=shash2(); int st=int(uState+0.5);
  if(uCalm>0.5){ if(uSnap>0.5){ if(st==0){ float l; p=heapTarget(h,h2,l);} else { vec2 hp=homePos(h); bool fig=(abs(uA.x)+abs(uA.y)+abs(uA.z)+abs(uA.w))>0.01; p=vec3((st==3&&fig)?nodal(hp)+(h2.zw-0.5)*0.045:hp,0.);} } gl_FragColor=vec4(p,1.); return; }
  p+=v*uDt;
  if(st>=2 && st!=1){ float r=length(p.xy); if(r<1.03 && p.z<0.35){ if(p.z<0.) p.z=0.; if(r>0.995) p.xy*=0.995/r; } }
  if(st==0){ float l; vec3 tg=heapTarget(h,h2,l); if(p.y<tg.y) p.y=tg.y; if(p.z<0.) p.z=0.; }
  gl_FragColor=vec4(p,1.);
}`;

/** Shared vertex logic: plate units → CSS px, with the organic edge warp (outer rim only). */
const PCOMMON = /* glsl */ `
uniform sampler2D tPos; uniform vec2 uC; uniform float uR, uTilt, uPx, uViewH, uSeed, uWarp, uScale;
attribute vec2 ref;
uvec3 pcg3d(uvec3 v){ v = v*1664525u + 1013904223u; v.x += v.y*v.z; v.y += v.z*v.x; v.z += v.x*v.y; v ^= v>>16u; v.x += v.y*v.z; v.y += v.z*v.x; v.z += v.x*v.y; return v; }
vec4 ghash(){ uvec3 a=pcg3d(uvec3(uvec2(ref*8192.0), 5u)); uvec3 b=pcg3d(uvec3(uvec2(ref*8192.0), 29u)); return vec4(vec3(a)*(1.0/4294967296.0), float(b.x)*(1.0/4294967296.0)); }
// Organic, uneven perimeter: a sum of low harmonics with per-session phases (deterministic: no flicker).
float edgeR(float th){ return 1.0 + 0.075*sin(3.0*th+uSeed) + 0.05*sin(5.0*th+uSeed*1.7) + 0.032*sin(8.0*th+uSeed*2.3) + 0.018*sin(13.0*th+uSeed*0.7) + 0.01*sin(21.0*th+uSeed*3.1); }
vec2 screenPos(vec3 p, vec4 h, out float persp, out float rim){
  float ct=cos(uTilt), stt=sin(uTilt); float yy=p.y*ct-p.z*stt; float zz=p.y*stt+p.z*ct;
  persp=3.2/(3.2-clamp(zz,-1.5,1.5));
  vec2 q=vec2(p.x,yy);
  float r=length(q); float th=atan(q.y,q.x);
  float k=smoothstep(0.6,1.0,r)*uWarp*step(r,1.08);          // core untouched: Chladni stays true there
  q*=mix(1.0, edgeR(th), k);
  float stray=step(h.w,0.08)*smoothstep(0.78,0.99,r)*uWarp*step(r,1.08); // stray grains spill over the rim
  q*=1.0 + stray*(0.03 + 0.26*h.y*h.y*h.y);
  rim=k;
  vec2 s=uC+vec2(q.x,-q.y)*uR*persp;
  return s;
}
`;

export const PVERT = PCOMMON + /* glsl */ `
uniform float uMat, uTime, uAlpha, uKeep, uCount, uViewW; uniform sampler2D tDens;
varying float vA; varying vec3 vCol; varying float vSpark;
void main(){
  vec3 p=texture2D(tPos,ref).xyz; vec4 h=ghash(); float persp, rim;
  vec2 s=screenPos(p,h,persp,rim);
  gl_Position=projectionMatrix*vec4(s.x*uScale, (uViewH-s.y)*uScale, 0., 1.);
  float size; vec3 col; float a; vSpark=0.0;
  if(uMat<0.5){ // SAND: pale warm grains, cream / beige / warm grey with per-grain hue and brightness
    size=1.9+2.1*h.x*h.x;
    col=mix(vec3(0.94,0.89,0.79), vec3(0.71,0.65,0.56), h.y);
    col=mix(col, vec3(0.84,0.81,0.76), step(0.7,h.z)*0.6);          // a few cooler grey grains
    col*=0.86+0.26*h.z;
    float dn=1.0-exp(-texture2D(tDens, vec2(s.x/uViewW, 1.0-s.y/uViewH)).r*0.3); // heap relief
    col*=0.8+0.26*dn;
    a=0.9+0.1*h.x;
    vSpark=step(h.w,0.025);                                         // a slight sparkle, still matte
    a*=1.0-0.35*step(0.9,length(p.xy))*rim*h.y;                     // edge thins out irregularly
  } else { // FIREFLIES: a sparse warm glow
    float keep=step(h.x,uKeep); size=keep*(4.0+4.5*h.y); col=mix(vec3(0.80,0.95,0.45),vec3(1.0,0.84,0.42),h.z);
    a=keep*(0.35+0.65*(0.5+0.5*sin(uTime*(0.8+1.6*h.z)+h.y*40.0)));
  }
  a*=(1.0+clamp(p.z,0.,0.3)*1.5)*uAlpha*(uMat>0.5?0.7:uCount);
  gl_PointSize=size*persp*uPx*(1.0+clamp(p.z,0.,0.3)*0.8);
  vA=a; vCol=col;
}`;

export const PFRAG = /* glsl */ `
uniform float uMat; uniform vec3 uLight; uniform float uLightK;
varying float vA; varying vec3 vCol; varying float vSpark;
void main(){ vec2 c=gl_PointCoord-0.5; float d=length(c)*2.0; if(d>1.0) discard;
  if(uMat>0.5){ float core=exp(-d*d*4.0)+0.4*exp(-d*d*1.4); gl_FragColor=vec4(vCol*core*vA, 0.0); return; }
  // each grain is a tiny matte sphere: lit side toward the canopy light, a darker underside
  vec3 n=vec3(c.x*2.0, -c.y*2.0, sqrt(max(0.0,1.0-d*d)));
  float dif=clamp(dot(n,uLight),0.0,1.0);
  float shade=0.48+0.62*dif*uLightK + 0.1*(1.0-uLightK);
  vec3 col=vCol*shade + vSpark*pow(dif,24.0)*0.55;
  float a=clamp(vA*smoothstep(1.0,0.7,d),0.0,1.0);
  gl_FragColor=vec4(col*a, a); }`;

/** Density splat into a low-res target, for the contact shadow. */
export const DVERT = PCOMMON + /* glsl */ `
uniform float uMat; varying float vD;
void main(){ vec3 p=texture2D(tPos,ref).xyz; vec4 h=ghash(); float persp, rim; vec2 s=screenPos(p,h,persp,rim);
  gl_Position=projectionMatrix*vec4(s.x*uScale, (uViewH-s.y)*uScale, 0., 1.);
  gl_PointSize=1.6; vD=(uMat>0.5?0.0:1.0)*(1.0-clamp(p.z*6.0,0.,0.8)); }`;
export const DFRAG = /* glsl */ `varying float vD; uniform float uGain; void main(){ vec2 c=gl_PointCoord-0.5; float d=length(c)*2.0; if(d>1.0) discard; gl_FragColor=vec4(vD*uGain*(1.0-d*d)); }`;

export const QVERT = `varying vec2 vUv; void main(){ vUv=uv; gl_Position=vec4(position.xy,0.,1.); }`;
/** Soft contact / ambient-occlusion shadow: a blurred density map, offset away from the light, drawn as a
 *  darker green (never black) under dense heaps. */
export const SHADOWFRAG = /* glsl */ `
uniform sampler2D tD; uniform vec2 uTexel, uOff; uniform float uK; uniform vec3 uShadow; varying vec2 vUv;
void main(){ vec2 uv=vUv+uOff; float s=0.0, w=0.0;
  for(int y=-3;y<=3;y++) for(int x=-3;x<=3;x++){ float k=exp(-float(x*x+y*y)/6.0); s+=(1.0-exp(-texture2D(tD, uv+vec2(float(x),float(y))*uTexel*2.4).r*uK))*k; w+=k; }
  float d=s/w; float a=0.3*d*(2.0-d); // soft falloff, never a hard plateau
  gl_FragColor=vec4(uShadow*a, a); }`;

/** Canopy dapples + mist (time of day light), rendered at 1/4 resolution. Additive light only. */
export const CANOPYFRAG = /* glsl */ `
uniform vec2 uRes; uniform float uTime, uDrift, uInt, uMist, uDpr; uniform vec2 uLight, uPar; uniform vec3 uLCol, uMistCol;
varying vec2 vUv;
float hash(vec2 p){ return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453); }
float noise(vec2 p){ vec2 i=floor(p), f=fract(p); f=f*f*(3.0-2.0*f); return mix(mix(hash(i),hash(i+vec2(1,0)),f.x), mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),f.x), f.y); }
float fbm(vec2 p){ float s=0., a=0.5; for(int i=0;i<5;i++){ s+=a*noise(p); p=p*2.03+vec2(1.7,9.2); a*=0.5; } return s; }
void main(){
  vec2 uv=vec2(vUv.x, 1.0-vUv.y); float asp=uRes.x/uRes.y;
  vec2 q=(uv+uPar)*vec2(asp,1.0)*2.4; float t=uTime*uDrift;
  float n=fbm(q+vec2(t*0.6,t*0.25))*0.65+fbm(q*2.3-vec2(t*0.3,t*0.5))*0.35;
  float spots=smoothstep(0.55,0.74,n);
  float fall=exp(-length((uv-uLight)*vec2(asp,1.0))*1.35);
  vec3 col=uLCol*(spots*0.85+0.22)*fall*uInt;
  float mn=fbm(vec2(uv.x*3.0*asp+uTime*0.018, uv.y*5.0-uTime*0.004));
  col+=uMistCol*smoothstep(0.78,1.0,uv.y)*(0.35+0.65*mn)*uMist;
  gl_FragColor=vec4(col,0.0);
}`;
export const BLITFRAG = `uniform sampler2D tC; varying vec2 vUv; void main(){ gl_FragColor=vec4(texture2D(tC,vUv).rgb,0.0); }`;
