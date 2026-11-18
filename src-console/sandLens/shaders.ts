import { LUT_W } from "./bessel";

/**
 * design-1118b sand shaders (ported from the Stage A mock, then re-looked per Taisun's FIGURA reference):
 * the same Bessel/Chladni physics; dense, heaped, matte cream grains in normal alpha blending (no glow),
 * per-grain diffuse from a fixed soft light with a darker underside, a soft contact shadow from a blurred
 * density map, and an organic uneven outer edge (per-session seed) that leaves the core physics untouched.
 */
export const COMMON = /* glsl */ `
uniform float uDt, uTime, uFrame, uState, uWakeT, uLoud, uMic, uBreathP, uCalm, uSnap, uStir;
uniform sampler2D uLut; uniform vec4 uRow, uN, uA, uPk;
uvec3 pcg3d(uvec3 v){ v = v*1664525u + 1013904223u; v.x += v.y*v.z; v.y += v.z*v.x; v.z += v.x*v.y; v ^= v>>16u; v.x += v.y*v.z; v.y += v.z*v.x; v.z += v.x*v.y; return v; }
vec3 rnd3(uvec3 v){ return vec3(pcg3d(v)) * (1.0/4294967296.0); }
vec4 shash(){ uvec2 c=uvec2(gl_FragCoord.xy); vec3 a=rnd3(uvec3(c,17u)); vec3 b=rnd3(uvec3(c,91u)); return vec4(a,b.x); }
vec4 shash2(){ uvec2 c=uvec2(gl_FragCoord.xy); vec3 a=rnd3(uvec3(c,233u)); vec3 b=rnd3(uvec3(c,377u)); return vec4(a,b.y); }
const float uSeedV=3.7;
float vh(vec2 i){ return fract(sin(dot(i,vec2(127.1,311.7)))*43758.5453); }
float vnoise(vec2 x){ vec2 i=floor(x), f=fract(x); f=f*f*(3.0-2.0*f); return mix(mix(vh(i),vh(i+vec2(1,0)),f.x), mix(vh(i+vec2(0,1)),vh(i+vec2(1,1)),f.x), f.y); }
float radial(float row, float r){ float x=clamp(r,0.,1.)*${(LUT_W - 1).toFixed(1)}; int i=int(floor(x)); int j=min(i+1,${LUT_W - 1}); float f=x-float(i);
  int ro=int(row+0.5); return mix(texelFetch(uLut, ivec2(i,ro),0).r, texelFetch(uLut, ivec2(j,ro),0).r, f); }
float field(vec2 p){ float r=length(p); float th=atan(p.y,p.x); float w=0.;
  for(int i=0;i<4;i++){ if(abs(uA[i])>1e-4) w+=uA[i]*radial(uRow[i],r)*cos(uN[i]*th); } return w; }
// what a grain feels: the same field, but past each mode's last antinode the plate acts as a free edge (no rim node)
float fieldFree(vec2 p){ float r=length(p); float th=atan(p.y,p.x); float w=0.;
  for(int i=0;i<4;i++){ if(abs(uA[i])>1e-4) w+=uA[i]*radial(uRow[i],min(r,uPk[i]))*(1.0+1.5*max(r-uPk[i],0.))*cos(uN[i]*th); } return w; } // slope keeps drifting grains moving inwards
float fieldG(vec2 p, bool rim){ return rim?field(p):fieldFree(p); }
// Golden-ratio character, felt rather than seen. Rest places follow a Vogel (sunflower) spiral, grain i at
// r = sqrt(i/N), angle i x 137.5 deg, jittered by most of a spacing so no spiral arms ever show.
const float PHI=1.6180339887, GOLDEN_ANGLE=2.3999632297;
vec2 homePos(vec4 h){
  vec2 c=floor(gl_FragCoord.xy); float n=resolution.x*resolution.y; float i=c.y*resolution.x+c.x;
  float k=i;
  float r=0.965*sqrt((k+0.5)/n); float a=k*GOLDEN_ANGLE;
  vec2 p=r*vec2(cos(a),sin(a));
  vec2 j=(h.xy-0.5)*1.6*(0.965/sqrt(n))*4.2;   // jitter ~ one spacing
  p+=j; float rr=length(p); if(rr>0.965) p*=0.965/rr; return p; }
// Idle morph: slow outline and density drift on incommensurate periods T, T*phi, T*phi^2 (never repeats),
// plus a faint swirl along a logarithmic golden spiral (r grows by phi per quarter turn).
vec2 idleMorph(vec2 hp, float t, float amt){
  float r=length(hp); float th=atan(hp.y,hp.x);
  const float T=9.0;
  float w1=6.2831853*t/T, w2=6.2831853*t/(T*PHI), w3=6.2831853*t/(T*PHI*PHI);
  // outline: low harmonics breathe at the rim much more than in the core
  float rim=r*r;
  float dr=rim*(0.028*sin(2.0*th+w1+1.3)*sin(w2*0.5+0.7) + 0.02*sin(3.0*th-w2+4.1) + 0.014*sin(5.0*th+w3+2.2));
  // density drift: a slow value-noise current, sampled on the phi periods
  vec2 q=hp*2.1+vec2(sin(w2)*0.6, cos(w3)*0.6);
  vec2 drift=(vec2(vnoise(q+vec2(1.7,9.2)), vnoise(q+vec2(8.3,2.8)))-0.5)*0.035;
  // golden-spiral swirl: a gentle back-and-forth turn d with radius scaled by phi^(d/(pi/2))
  float d=0.05*sin(w3+0.4)+0.03*sin(w1*PHI);
  float rs=(r+dr)*pow(PHI, d/1.5707963*0.35);
  vec2 p=rs*vec2(cos(th+d*(1.0-0.5*r)), sin(th+d*(1.0-0.5*r)))+drift*(0.4+r);
  return mix(hp, p, amt); }
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
  if(st==1){ // WAKE: the floating layer stirs in place: a soft golden-spiral turn and bloom, then settles (~2 s)
    float t=uWakeT; float b=sin(3.1416*clamp(t/0.95,0.,1.));
    vec2 hp=homePos(h); float r=length(hp); float d=0.55*b*(1.0-0.6*r);
    vec2 sw=hp*pow(PHI, d/1.5707963*0.25); sw=vec2(sw.x*cos(d)-sw.y*sin(d), sw.x*sin(d)+sw.y*cos(d));
    vec3 tg=vec3(sw*(1.0+0.06*b), (0.08+0.25*h.z)*b*0.4);
    v+=((tg-p)*9.0-v*5.0)*dt;
  } else if(st==2 || st==0){ // LISTENING / VOICE OFF: an even floating layer, softly breathing and slowly morphing
    vec2 hp=idleMorph(homePos(h), uTime, 1.0); float br=sin(uTime*6.2831853/uBreathP + length(hp)*2.2);
    vec3 tg=vec3(hp*(1.0+0.012*br), 0.025*br*(0.5+h.z));
    v+=((tg-p)*7.0-v*5.0)*dt + (rn-0.5)*vec3(1.,1.,0.4)*uMic*1.2*dt*20.0;
  } else { // SPEAKING: Chladni physics. Grains hop where |w| is large and come to rest on the nodal lines.
    float r=length(p.xy);
    if(r>1.03 || p.z>0.35){ v+=((vec3(homePos(h),0.)-p)*8.0-v*4.0)*dt; }
    else if(p.z<=0.0008){
      // A vibrating plate tosses grains up where it moves (|w| large); each bounce lands a little further
      // down the |w| slope, so grains drain off the antinodes and rest on the nodal lines (|w| ≈ 0).
      const float e=0.004;
      // the clamped rim is a nodal line too; only a few grains rest there (the organic edge), the rest feel a free edge
      bool rim=h.w<0.12;
      float w=fieldG(p.xy,rim); float aw=abs(w); float L=uLoud;
      vec2 g=vec2(fieldG(p.xy+vec2(e,0.),rim)-fieldG(p.xy-vec2(e,0.),rim), fieldG(p.xy+vec2(0.,e),rim)-fieldG(p.xy-vec2(0.,e),rim))/(2.0*e);
      vec2 down=-sign(w)*g; float gl=length(down); vec2 dir=gl>1e-4?down/gl:vec2(0.);
      float dn=aw/max(length(g),1e-3); // distance to the nearest nodal line: quiet regions of high-n modes still clear
      // Heaped bands, not threads: each grain settles anywhere inside its own tolerance of the nodal line
      // (most close to it, a few out at the shoulder), and the band width wanders along the line so it clumps.
      float hb=rnd3(uvec3(uvec2(gl_FragCoord.xy),611u)).x;
      float lump=0.55+0.9*vnoise(p.xy*9.0+vec2(uSeedV,1.7))*vnoise(p.xy*23.0+vec2(4.1,uSeedV));
      float tol=0.012+0.095*pow(hb,1.25)*lump;
      float act=smoothstep(tol,tol+0.012,dn)*smoothstep(0.002,0.01,aw)*L; // inside its band a grain is still
      if(uStir>0.6 && rn.x<0.85){ // pitch change: the plate's attack tosses the whole layer, so every line of the new figure fills
        float vz=(0.5+0.4*rn2.x); float fl=2.0*vz/40.0; // each toss lands about half-way back to the grain's place in an even layer
        vec2 tv=(homePos(h)-p.xy)*0.5/fl; float tl=length(tv); v=vec3(tl>1.6?tv*(1.6/tl):tv, vz); // capped: one frame never jumps the rim
      } else if(rn.x < clamp(act*2.5,0.,1.)){
        float an=rn.y*6.2831853; float hs=(0.5+0.7*aw)*L*(1.0+2.0*smoothstep(0.02,0.1,dn)); // far from a line the grains travel faster, close to it they creep in
        vec2 hv=dir*(0.55+0.35*rn.z)*hs + vec2(cos(an),sin(an))*0.4*hs*rn2.y; // mostly downhill, a little scatter
        v=vec3(hv, (0.35+0.9*aw)*L*(0.7+0.6*rn2.x));
      } else { v=vec3(0.); }
    } else { v.z-=40.0*dt; v.xy*=(1.0-1.2*dt); if(r>0.985 && dot(v.xy,p.xy)>0.) v.xy=reflect(v.xy,p.xy/r)*0.5; }
  }
  gl_FragColor=vec4(v,0.);
}`;

export const POS = COMMON + /* glsl */ `
void main(){
  vec2 uv=gl_FragCoord.xy/resolution.xy;
  vec3 p=texture2D(texturePosition,uv).xyz; vec3 v=texture2D(textureVelocity,uv).xyz;
  vec4 h=shash(), h2=shash2(); int st=int(uState+0.5);
  if(uCalm>0.5){ if(uSnap>0.5){ vec2 hp=homePos(h); bool fig=(abs(uA.x)+abs(uA.y)+abs(uA.z)+abs(uA.w))>0.01; p=vec3((st==3&&fig)?nodal(hp)+(h2.zw-0.5)*0.045:hp,0.); } gl_FragColor=vec4(p,1.); return; }
  p+=v*uDt;
  if(st!=1){ float r=length(p.xy); if(r<1.03 && p.z<0.35){ if(p.z<0.) p.z=0.; if(r>0.995) p.xy*=0.995/r; } }
  gl_FragColor=vec4(p,1.);
}`;

/** Shared vertex logic: plate units → CSS px, with the organic edge warp (outer rim only). */
const PCOMMON = /* glsl */ `
uniform sampler2D tPos; uniform vec2 uC; uniform float uR, uTilt, uPx, uViewH, uSeed, uWarp, uScale, uBeat, uGlow;
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
  float k=smoothstep(0.86,1.0,r)*uWarp*step(r,1.08);         // outer rim only: the Chladni figure inside stays true
  q*=1.0 + k*max(edgeR(th)-1.0, -0.035);                      // bulges out freely, pulls in only a little: lines crossing the rim never bunch up
  float stray=step(h.w,0.08)*smoothstep(0.9,0.99,r)*uWarp*step(r,1.08); // stray grains spill over the rim
  q*=1.0 + stray*(0.03 + 0.26*h.y*h.y*h.y);
  rim=k;
  vec2 s=uC+vec2(q.x,-q.y)*uR*persp*(1.0+uBeat); // heartbeat: a lift of a few per cent
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
    size=2.3+2.6*h.x*h.x;
    col=mix(vec3(0.94,0.89,0.79), vec3(0.71,0.65,0.56), h.y);
    col=mix(col, vec3(0.84,0.81,0.76), step(0.7,h.z)*0.6);          // a few cooler grey grains
    col*=0.86+0.26*h.z;
    // heap relief: the density map is a height field; its slope gives lit tops and shadowed sides
    vec2 du=vec2(s.x/uViewW, 1.0-s.y/uViewH), px=vec2(5.0/uViewW, 5.0/uViewH);
    float H0=1.0-exp(-texture2D(tDens,du).r*0.3);
    float Hx=(1.0-exp(-texture2D(tDens,du+vec2(px.x,0.)).r*0.3))-(1.0-exp(-texture2D(tDens,du-vec2(px.x,0.)).r*0.3));
    float Hy=(1.0-exp(-texture2D(tDens,du+vec2(0.,px.y)).r*0.3))-(1.0-exp(-texture2D(tDens,du-vec2(0.,px.y)).r*0.3));
    vec3 hn=normalize(vec3(-Hx*4.0, -Hy*4.0, 1.0));
    float lit=dot(hn, normalize(vec3(-0.45,0.55,0.7)));
    col*=(0.66+0.34*H0)*(0.62+0.58*clamp(lit,0.,1.));
    col*=1.0+uGlow;                                                   // heartbeat: brightness swell
    a=0.9+0.1*h.x;
    vSpark=step(h.w,0.025);                                         // a slight sparkle, still matte
    a*=1.0-0.35*step(0.9,length(p.xy))*rim*h.y;                     // edge thins out irregularly
  } else { // FIREFLIES: a sparse warm glow
    float keep=step(h.x,uKeep); size=keep*(4.0+4.5*h.y); col=mix(vec3(0.80,0.95,0.45),vec3(1.0,0.84,0.42),h.z);
    a=keep*(0.35+0.65*(0.5+0.5*sin(uTime*(0.8+1.6*h.z)+h.y*40.0)));
  }
  a*=(1.0+clamp(p.z,0.,0.3)*1.5)*uAlpha*(uMat>0.5?0.7:uCount);
  gl_PointSize=size*persp*uPx*(1.0+clamp(p.z,0.,0.3)*0.8)*(1.0+uBeat*2.5); // heartbeat: density swell
  vA=a; vCol=col;
}`;

export const PFRAG = /* glsl */ `
uniform float uMat; uniform vec3 uLight; uniform float uLightK;
varying float vA; varying vec3 vCol; varying float vSpark;
void main(){ vec2 c=gl_PointCoord-0.5; float d=length(c)*2.0; if(d>1.0) discard;
  if(uMat>0.5){ float core=exp(-d*d*4.0)+0.4*exp(-d*d*1.4); gl_FragColor=vec4(vCol*core*vA, 0.0); return; }
  // each grain is a tiny matte sphere: lit side toward the light, a darker underside
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
  float d=s/w; float a=0.4*d*(2.0-d); // soft falloff, never a hard plateau
  gl_FragColor=vec4(uShadow*a, a); }`;
