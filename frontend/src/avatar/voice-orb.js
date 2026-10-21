// Voice Orb / Ship Notes. Local audio analysis, no dependencies.
(() => {
  if (typeof window === 'undefined' || typeof customElements === 'undefined' || customElements.get('voice-orb')) return;
  const STATES = ['idle', 'listening', 'thinking', 'speaking', 'text'];
  // All particles pure white across ALL states:
  const PALETTE = [[1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 1.0]];
  const mediaSources = new WeakMap();
  const clamp = (v, lo=0, hi=1) => Math.max(lo, Math.min(hi, Number.isFinite(+v) ? +v : lo));
  const weightsFor = state => STATES.map(s => +(s === state));
  const parseColorHex = _hex => [1.0, 1.0, 1.0];
  const normalize = w => {
    const a = Array.from({length:5}, (_,i) => clamp(w?.[i]));
    const sum = a.reduce((s,v) => s+v, 0);
    return sum ? a.map(v => v/sum) : [1,0,0,0,0];
  };
  const seeded = i => { const s = Math.sin(i*127.1+311.7)*43758.5453; return s-Math.floor(s); };
  const hash3 = (x, y, z) => {
    const s = Math.sin(x*127.1+y*311.7+z*74.7)*43758.5453;
    return s-Math.floor(s);
  };
  const smooth01 = x => x*x*(3-2*x);
  const mixNum = (a, b, t) => a+(b-a)*t;
  const vnoise = (x, y, z) => {
    const xi=Math.floor(x), yi=Math.floor(y), zi=Math.floor(z);
    const xf=smooth01(x-xi), yf=smooth01(y-yi), zf=smooth01(z-zi);
    const a=hash3(xi,yi,zi), b=hash3(xi+1,yi,zi);
    const c=hash3(xi,yi+1,zi), d=hash3(xi+1,yi+1,zi);
    const e=hash3(xi,yi,zi+1), f=hash3(xi+1,yi,zi+1);
    const g=hash3(xi,yi+1,zi+1), h=hash3(xi+1,yi+1,zi+1);
    return mixNum(mixNum(mixNum(a,b,xf),mixNum(c,d,xf),yf),mixNum(mixNum(e,f,xf),mixNum(g,h,xf),yf),zf)*2.0-1.0;
  };
  const normalize3 = (x, y, z) => { const l = Math.hypot(x,y,z)||1; return [x/l,y/l,z/l]; };
  const cross3 = (ax,ay,az,bx,by,bz) => [ay*bz-az*by, az*bx-ax*bz, ax*by-ay*bx];
  const sphere = count => {
    const data = new Float32Array(count*4);
    const jitter = .55 / Math.sqrt(count);
    for (let i=0; i<count; i++) {
      const y = clamp(1-2*(i+.5)/count+(seeded(i+7)-.5)*jitter, -.99999, .99999);
      const a = i*2.399963229728653+(seeded(i+19)-.5)*jitter*5;
      const r = Math.sqrt(1-y*y);
      data.set([r*Math.cos(a), y, r*Math.sin(a), i/count], i*4);
    }
    return data;
  };
  // Ghost enter: scattered start positions on a wide edge-biased ring —
  // particles burst inward from around the rim, never from dead center.
  const scatterOf = count => {
    const data = new Float32Array(count*3);
    for (let i=0; i<count; i++) {
      const a = seeded(i+53)*6.2831853;
      const r = 1.8 + seeded(i+71)*1.2;
      data.set([r*Math.cos(a), (seeded(i+97)-.5)*2.6, r*Math.sin(a)], i*3);
    }
    return data;
  };
  // ─── Particle-generated text (Feature 88) ─────────────────────────
  // Rasterize the text on an offscreen 2D canvas, sample glyph pixels,
  // and map them into orb space. Returns null when the text renders
  // empty (also the no-canvas graceful path).
  const sampleTextPoints = text => {
    if (typeof document === 'undefined') return null;
    const t = String(text ?? '').trim().slice(0, 16);
    if (!t) return null;
    const W = 360, H = 128;
    const c = document.createElement('canvas');
    c.width = W; c.height = H;
    const ctx = c.getContext('2d', { willReadFrequently: true });
    if (!ctx) return null;
    ctx.fillStyle = '#fff';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    // Fit the text to the canvas instead of a fixed size: at a flat 54px
    // bold, anything past ~5-6 characters overflowed the 360px canvas and
    // got silently clipped at BOTH edges (text is center-anchored), e.g.
    // "Right away." rendered as "ght awa" — only the surviving middle
    // slice. Measure at the reference size, then scale down (never up) so
    // the full phrase — up to the 16-char cap above — stays on-canvas.
    const refSize = 54;
    ctx.font = `bold ${refSize}px "Segoe UI", system-ui, sans-serif`;
    const measured = ctx.measureText(t).width;
    const maxWidth = W * 0.70;
    const fontSize = measured > maxWidth ? Math.max(20, Math.floor(refSize * (maxWidth / measured))) : refSize;
    ctx.font = `bold ${fontSize}px "Segoe UI", system-ui, sans-serif`;
    ctx.fillText(t, W/2, H/2 + 4);
    const img = ctx.getImageData(0, 0, W, H).data;
    const pts = [];
    for (let y=0; y<H; y+=1) {
      for (let x=0; x<W; x+=1) {
        if (img[(y*W+x)*4+3] > 110) pts.push([x/W, 1-y/H]);
      }
    }
    return pts.length ? pts : null;
  };
  // Assign every particle a text target: glyph points get 2-4 particles
  // each (volume), the rest precompute positions on a wide faint ambient
  // halo (flag w >= 0.5 — the shader dims them and never converges them).
  const textOf = (count, pts) => {
    const data = new Float32Array(count*4);
    if (!pts || !pts.length) {
      for (let i=0; i<count; i++) {
        const a = seeded(i+211)*6.2831853;
        const r = 1.42 + seeded(i+233)*0.5;
        data.set([r*Math.cos(a), (seeded(i+251)-.5)*1.4, r*Math.sin(a), 0.5+seeded(i+269)*0.5], i*4);
      }
      return data;
    }
    const nText = Math.round(count * clamp(pts.length*4/count, 0.30, 0.88));
    for (let i=0; i<count; i++) {
      if (i < nText) {
        const p = pts[(i*7919) % pts.length];
        data.set([
          (p[0]-.5)*1.72,
          (p[1]-.5)*0.64+0.02,
          (seeded(i+131)-.5)*0.16,
          seeded(i+173)*0.49,
        ], i*4);
      } else {
        const a = seeded(i+211)*6.2831853;
        const r = 1.42 + seeded(i+233)*0.5;
        data.set([r*Math.cos(a), (seeded(i+251)-.5)*1.4, r*Math.sin(a), 0.5+seeded(i+269)*0.5], i*4);
      }
    }
    return data;
  };

  const VS = `
  precision highp float;
  attribute vec4 seed;
  attribute vec3 scatter;
  attribute vec4 textPos;
  uniform float time, pixels, density, onset, reduced, assemble, tw, textProg, thinkIn;
  uniform mediump float glitch;
  uniform vec4 weights;
  uniform vec3 bands;
  uniform vec3 textTint;
  uniform vec3 userTint;
  uniform int u_think_mode;
  uniform int u_speak_mode;
  uniform int u_listen_mode;
  varying vec3 tint;
  varying float strength, spark;
  varying vec3 sparkTint;
  varying float crisp;
  float hash(vec3 p) { return fract(sin(dot(p,vec3(127.1,311.7,74.7)))*43758.5453); }
  float noise(vec3 p) {
    vec3 i=floor(p), f=fract(p); f=f*f*(3.0-2.0*f);
    float a=mix(hash(i),hash(i+vec3(1,0,0)),f.x);
    float b=mix(hash(i+vec3(0,1,0)),hash(i+vec3(1,1,0)),f.x);
    float c=mix(hash(i+vec3(0,0,1)),hash(i+vec3(1,0,1)),f.x);
    float d=mix(hash(i+vec3(0,1,1)),hash(i+vec3(1,1,1)),f.x);
    return mix(mix(a,b,f.y),mix(c,d,f.y),f.z)*2.0-1.0;
  }
  vec3 turn(vec3 p,float a) { float c=cos(a),s=sin(a);return vec3(c*p.x+s*p.z,p.y,c*p.z-s*p.x); }
  vec3 rotate3D(vec3 p,float yaw,float pitch) {
    float cy=cos(yaw),sy=sin(yaw);
    float cp=cos(pitch),sp=sin(pitch);
    vec3 p1=vec3(cy*p.x+sy*p.z,p.y,cy*p.z-sy*p.x);
    return vec3(p1.x,cp*p1.y-sp*p1.z,cp*p1.z+sp*p1.y);
  }
  void main() {
    float t=time;
    vec3 n=seed.xyz;
    float angle=acos(clamp(n.z,-1.0,1.0));
    float drift=noise(n*2.7+vec3(t*.19,-t*.11,t*.08));
    float bass=noise(n*1.8+vec3(t*.32,0,-t*.2));
    float grain=noise(n*17.0+vec3(-t*1.8,t*.7,t));
    float low=bands.x, mid=bands.y, high=bands.z;
    // Active 50/50 Listening State (Acoustic Sand Cymatics & Soundbar Beat Motion):
    // 50% particles form a rigid, crystalline anchor shell at r = 1.0 (Fibonacci parity).
    // The other 50% act like sand grains on an acoustic soundbar / Chladni plate:
    // When silent, sand grains REST AT r = 1.0 (zero thread loops, zero continuous line waves).
    // ONLY when user speaks / audio hits, grains bounce and scatter randomly in and out to voice beats.
    bool isFixed=mod(floor(seed.w*6240.0+0.5),2.0)<0.5;
    float soundEnergy=max(low,max(mid,high))*0.75+onset*0.85;
    float voiceLevel=clamp((soundEnergy-0.02)/0.98,0.0,1.0);
    float h1=fract(sin(dot(n,vec3(43.7,89.3,157.1))+seed.w*183.1)*43758.5453);
    float h2=fract(sin(dot(n,vec3(71.9,131.7,29.3))+seed.w*241.7)*31415.9265);
    float h3=fract(sin(dot(n,vec3(113.3,17.7,83.1))+seed.w*317.9)*27182.8182);
    float bounce=sin(t*(18.0+34.0*h1)+h2*6.2831853);
    float beatKick=(h1-0.5)*2.0*onset*0.45;
    float jitter=(h3-0.5)*0.16*mid;
    float dynR=1.0+voiceLevel*(0.35*bounce+beatKick+jitter);
    float listenR=isFixed?1.0:dynR;

    // Speaking: Dynamic Morphology (Option #2 = 64-strand Fibonacci Blossom with petal pulses)
    float fluidNoise1=noise(n*0.92+vec3(t*0.20,-t*0.15,t*0.12));
    float textDamp=1.0-0.75*textProg;
    float speechBulge=(0.18*max(low,mid)+0.32*onset)*textDamp;
    float dropSag=-0.08*clamp(-n.y,0.0,1.0)*(1.0-0.4*n.x*n.x);
    float bloom = (u_speak_mode == 2 || u_speak_mode == 0) ? (sin(length(n.xy)*6.0 - t*3.0) * 0.22 * soundEnergy) : 0.0;
    float speakRad=1.0 + 0.12*fluidNoise1 + speechBulge*fluidNoise1 + dropSag + bloom;
    vec3 speakV=turn(n,t*0.18)*speakRad;

    // Thinking: Dynamic Morphology (Options 1..12, default 6 Trefoil Celtic Helix Cage)
    vec3 thought = vec3(0.0);
    if (u_think_mode == 1) {
      // #1 Torus Knot
      float ku = seed.w * 6.2831853;
      float knotR = 0.88 * (1.0 + 0.38*cos(3.0*ku + t*0.40));
      vec3 knotC = vec3(knotR * cos(2.0*ku + t*0.26), knotR * sin(2.0*ku + t*0.26), -0.54 * sin(3.0*ku + t*0.40));
      thought = turn(knotC + n*0.08, t*0.4) * mix(0.70, 1.0, thinkIn);
    } else if (u_think_mode == 2) {
      // #2 Neural Starburst
      float sId = floor(seed.w * 64.0);
      float sAlong = fract(seed.w * 64.0);
      float sy = 1.0 - 2.0*(sId + 0.5)/64.0;
      float sr = sqrt(max(0.0, 1.0 - sy*sy));
      float sang = sId * 2.399963;
      vec3 sDir = vec3(sr*cos(sang), sy, sr*sin(sang));
      thought = turn(sDir * pow(sAlong, 1.6) * 1.85 + n*0.08, t*0.35) * mix(0.70, 1.0, thinkIn);
    } else if (u_think_mode == 3) {
      // #3 Mobius Infinity
      float u = seed.w * 12.56637;
      float scale = 1.25 / (1.0 + sin(u*0.5)*sin(u*0.5));
      vec3 infC = vec3(scale * cos(u*0.5), scale * sin(u*0.5) * cos(u*0.5), 0.35 * sin(u + t*0.8));
      thought = turn(infC + n*0.08, t*0.45) * mix(0.70, 1.0, thinkIn);
    } else if (u_think_mode == 4) {
      // #4 Quantum Gyro
      float ring = floor(seed.w * 3.0);
      float a = fract(seed.w * 3.0) * 6.2831853;
      vec3 rP = vec3(cos(a), sin(a), (fract(seed.w*123.0)-0.5)*0.10);
      if (ring == 0.0) rP = vec3(rP.x, rP.y*cos(t*1.2) - rP.z*sin(t*1.2), rP.y*sin(t*1.2) + rP.z*cos(t*1.2));
      if (ring == 1.0) rP = vec3(rP.x*cos(t*1.5) + rP.z*sin(t*1.5), rP.y, -rP.x*sin(t*1.5) + rP.z*cos(t*1.5));
      if (ring == 2.0) rP = vec3(rP.x*cos(t*0.9) - rP.y*sin(t*0.9), rP.x*sin(t*0.9) + rP.y*cos(t*0.9), rP.z);
      thought = turn(rP * 1.15, t*0.2) * mix(0.70, 1.0, thinkIn);
    } else if (u_think_mode == 5) {
      // #5 Accretion Vortex
      float prog = fract(seed.w * 3.0);
      float sR = pow(prog, 0.65) * 1.55;
      float sA = prog * 12.566 + floor(seed.w*3.0)*2.094 - t*1.6;
      vec3 vP = vec3(sR*cos(sA), sR*sin(sA), (prog-0.5)*0.45*cos(sA));
      thought = turn(vec3(vP.x, vP.y*cos(0.5) - vP.z*sin(0.5), vP.y*sin(0.5) + vP.z*cos(0.5)), t*0.3) * mix(0.70, 1.0, thinkIn);
    } else if (u_think_mode == 7) {
      // #7 DNA Helix
      float strand = floor(seed.w * 2.0);
      float hZ = (fract(seed.w * 2.0) - 0.5) * 2.4;
      float hAng = hZ * 3.5 + t*1.5 + strand*3.14159;
      vec3 hP = vec3(0.65*cos(hAng), hZ, 0.65*sin(hAng));
      thought = turn(vec3(hP.x, hP.y*cos(0.4) - hP.z*sin(0.4), hP.y*sin(0.4) + hP.z*cos(0.4)), t*0.35) * mix(0.70, 1.0, thinkIn);
    } else if (u_think_mode == 8) {
      // #8 4D Tesseract
      float vId = floor(seed.w * 16.0);
      float x4 = mod(vId, 2.0)*2.0 - 1.0;
      float y4 = mod(floor(vId/2.0), 2.0)*2.0 - 1.0;
      float z4 = mod(floor(vId/4.0), 2.0)*2.0 - 1.0;
      float w4 = mod(floor(vId/8.0), 2.0)*2.0 - 1.0;
      float ang4 = t * 0.8;
      float rx = x4*cos(ang4) - w4*sin(ang4);
      thought = turn(vec3(rx, y4, z4)*0.6 + n*0.08, t*0.3) * mix(0.70, 1.0, thinkIn);
    } else if (u_think_mode == 9) {
      // #9 Hydro Jellyfish
      float isBell = step(0.35, seed.w);
      vec3 bellP = vec3(n.x * 1.2, abs(n.y)*0.8, n.z * 1.2);
      vec3 tentP = vec3(n.x*0.4, -abs(n.y)*1.4 - 0.2, n.z*0.4);
      thought = turn(mix(tentP, bellP, isBell), t*0.25) * mix(0.70, 1.0, thinkIn);
    } else if (u_think_mode == 10) {
      // #10 Magnetic Dynamo
      float theta = acos(clamp(n.y, -1.0, 1.0));
      float magR = 1.25 * sin(theta)*sin(theta);
      thought = turn(vec3(magR*cos(seed.w*6.28 + t*0.8), n.y*1.2, magR*sin(seed.w*6.28 + t*0.8)), t*0.3) * mix(0.70, 1.0, thinkIn);
    } else if (u_think_mode == 11) {
      // #11 Geodesic Crystal
      vec3 fN = normalize(floor(n * 2.5 + 0.5));
      vec3 cP = fN * 0.9 + n*0.1;
      thought = turn(vec3(cP.x*cos(t*0.4) - cP.y*sin(t*0.4), cP.x*sin(t*0.4) + cP.y*cos(t*0.4), cP.z), t*0.3) * mix(0.70, 1.0, thinkIn);
    } else if (u_think_mode == 12) {
      // #12 Solar Nebula
      float isRing = step(0.30, seed.w);
      float rRad = 0.8 + fract(seed.w*7.0)*0.8;
      float rAng = seed.w*20.0 + t*0.7;
      vec3 ringP = vec3(rRad*cos(rAng), 0.0, rRad*sin(rAng));
      ringP = vec3(ringP.x, ringP.y*cos(0.45) - ringP.z*sin(0.45), ringP.y*sin(0.45) + ringP.z*cos(0.45));
      thought = turn(mix(n*0.45, ringP, isRing), t*0.25) * mix(0.70, 1.0, thinkIn);
    } else {
      // #6 Trefoil Celtic Helix Cage (Default & User selection)
      float tStrand = floor(seed.w * 3.0);
      float tAng = fract(seed.w * 3.0) * 6.2831853;
      float tPhase = tStrand * 2.0943951; // 120 degrees
      float tR = 0.95 + 0.15*cos(3.0*tAng + t*1.2);
      vec3 tRingPos = vec3(tR*cos(tAng + t*0.5), tR*sin(tAng + t*0.5), 0.35*sin(2.0*tAng + tPhase));
      if (tStrand == 1.0) tRingPos = vec3(tRingPos.x, tRingPos.y*cos(1.05) - tRingPos.z*sin(1.05), tRingPos.y*sin(1.05) + tRingPos.z*cos(1.05));
      if (tStrand == 2.0) tRingPos = vec3(tRingPos.x*cos(1.05) - tRingPos.y*sin(1.05), tRingPos.x*sin(1.05) + tRingPos.y*cos(1.05), tRingPos.z);
      thought = turn(tRingPos + n*0.06, t*0.35) * mix(0.70, 1.0, thinkIn);
    }

    // Cloud position in the CURRENT base state (weights renormalized in JS
    // to sum 1 across the four base states; the text weight is separate).
    vec3 cloudPos=turn(n,t*.14)*listenR*(weights.x+weights.y)
      +thought*weights.z
      +speakV*weights.w;

    // Intact avatar: all particles remain in their organic morph geometry at all times (zero particle text)
    vec3 pos=cloudPos;

    float active=weights.y+weights.w;
    float ambF=0.0;
    float rim=pow(max(0.0,1.0-abs(n.z)),2.2);
    float pop=pow(max(0.0,sin(t*8.0+seed.w*149.0)),18.0)*step(.90,seed.w);
    pos*=1.0+active*high*pop*.17;
    // Intact sphere: particles maintain cohesive form without scattered explosion
    vec3 flight=pos;
    // Pure neat minimalist motion: zero glitch jitter or tearing
    float flow=pow(.5+.5*sin(angle*13.0+(weights.y-weights.w)*t*5.8+drift*2.0),7.0);
    float depth=clamp((flight.z+1.35)/2.7,0.0,1.0);
    float perspective=3.8/(3.8-flight.z*.60);
    gl_Position=vec4(flight.xy*perspective*.72,0,1);

    float point=(1.14+0.84*depth+0.42*rim)*mix(1.0,.72,weights.w)*density;
    point*=mix(1.0,.62,ambF);
    point+=active*high*pop*2.7;
    gl_PointSize=max(1.6,point*pixels/480.0);
    float cool=.5+.5*sin(n.y*2.1+n.x*1.6+drift*.65);
    float speakFace=clamp(n.z*.5+.5,0.0,1.0);
    float speakBump=clamp(.5+.5*fluidNoise1,0.0,1.0);

    // Pure bright white stardust particles across ALL states (idle, listening, thinking, speaking)
    tint=vec3(1.0,1.0,1.0);
    crisp=max(max(weights.w,tw),(weights.x+weights.y)*(isFixed?0.65:0.10));
    sparkTint=vec3(1.0,1.0,1.0);

    float uAlongKnot=fract(seed.w+t*0.06);
    float ribbonSpark=pow(max(0.0,sin(uAlongKnot*12.566-t*4.0)),10.0)*step(0.35,depth);
    strength=(.24+.45*depth+.70*rim)*(.65+.35*fract(sin(seed.w*912.7+31.4)*43758.5));
    float listenInGlow=(isFixed?0.0:1.0)*clamp((1.0-dynR)/0.35,0.0,1.0)*0.55;
    strength+=(weights.x+weights.y)*(listenInGlow+mid*flow*.40+onset*rim*.45);
    strength+=weights.w*(mid*flow*.95+onset*rim*.9);
    strength+=weights.z*(0.65+0.45*depth+ribbonSpark*1.6);
    strength+=weights.w*(0.50+0.40*fluidNoise1);
    strength+=tw*(.40+.40*depth);
    strength+=weights.w*.10*sin(t*.9+seed.w*6.2831);
    strength*=mix(1.0,0.95,weights.y);
    strength*=mix(1.0,1.02,weights.z);
    strength*=mix(1.0,1.18,weights.w);
    float listenSpark=(isFixed?0.0:1.0)*clamp((1.0-dynR)/0.35,0.0,1.0)*voiceLevel*0.65*(mid+onset);
    spark=active*(high*pop*.8+onset*rim*.22)+weights.z*(ribbonSpark*1.4+step(0.85,depth)*0.15)+(weights.x+weights.y)*listenSpark;
  }`;
  const FS = `
  precision mediump float;
  varying vec3 tint;
  varying float strength, spark;
  varying vec3 sparkTint;
  varying float crisp;
  uniform mediump float glitch;
  uniform float ink;
  void main() {
    float r=length(gl_PointCoord-.5)*2.0;
    if(r>1.0)discard;
    float edge=mix(.64,.30,crisp);
    float core=1.0-smoothstep(mix(.18,.08,crisp),edge,r);
    float halo=mix(exp(-r*r*4.0)*.24,exp(-r*r*8.0)*.12,crisp)*(1.0-smoothstep(.75,1.0,r));
    float a=(core+halo)*strength;
    vec3 color=vec3(1.0,1.0,1.0)*a+vec3(1.0,1.0,1.0)*spark*core;
    if(ink>0.5){
      // Light theme: black ink. The additive-white intensity becomes coverage
      // (premultiplied black = vec3(0)), composited with normal alpha blending.
      float lum=max(color.r,max(color.g,color.b));
      gl_FragColor=vec4(0.0,0.0,0.0,min(1.0,lum*1.1));
    } else {
      gl_FragColor=vec4(color,min(1.0,a+spark*core));
    }
  }`;

  class VoiceOrb extends HTMLElement {
    static get observedAttributes() { return ['state','particles','recording','color','theme','think-mode','speak-mode','listen-mode']; }
    get userColor() { return parseColorHex(this.getAttribute('color')); }
    constructor() {
      super();
      this._weights=[1,0,0,0,0]; this._bands=[0,0,0]; this._time=0; this._last=0;
      this._onset=0; this._bassHistory=0; this._slow=0; this._frame=0;
      this._assemble=1; this._assembleTarget=1; this._assembleFrom=1;
      this._assembleStart=0; this._assembling=false;
      // Particle-text phase machine (converge → hold → dissolve → null).
      this._textPhase=null; this._textProg=0; this._textStart=0;
      this._textHold=1600; this._prevState='idle'; this._textPts=null;
      this._textPending=null; this._textPosArr=null;
      this._thinkStart=0; this._thinkIn=1;
      // Glitch transition: a short burst fired whenever the dominant base
      // state (idle/listening/thinking/speaking) changes (see _tick).
      this._lastDom=null; this._glitchStart=0; this._glitchAmt=0;
      this._thinkMode = 6;
      this._speakMode = 2;
      this._listenMode = 1;
      try {
        const m = localStorage.getItem('nexus_orb_morphology');
        if (m) {
          const parsed = JSON.parse(m);
          if (parsed.thinking) this._thinkMode = parsed.thinking;
          if (parsed.speaking) this._speakMode = parsed.speaking;
          if (parsed.listening) this._listenMode = parsed.listening;
        }
      } catch (_) {}
      this._tick=this._tick.bind(this);
      this._sync=this._sync.bind(this);
    }
    connectedCallback() {
      try {
        import('@tauri-apps/api/core').then(({ invoke }) => {
          invoke('get_settings').then((s) => {
            if (s) {
              if (s.orbThinkMode) this._thinkMode = Number(s.orbThinkMode);
              if (s.orbSpeakMode) this._speakMode = Number(s.orbSpeakMode);
              if (s.orbListenMode) this._listenMode = Number(s.orbListenMode);
            }
          }).catch(() => {});
        }).catch(() => {});
      } catch (_) {}
      try {
        import('@tauri-apps/api/event').then(({ listen }) => {
          listen('orb:morphology_changed', (ev) => {
            if (ev.payload) {
              if (ev.payload.think) this._thinkMode = Number(ev.payload.think);
              if (ev.payload.speak) this._speakMode = Number(ev.payload.speak);
              if (ev.payload.listen) this._listenMode = Number(ev.payload.listen);
            }
          }).catch(() => {});
        }).catch(() => {});
      } catch (_) {}
      window.addEventListener('nexus:orb_morphology', (ev) => {
        if (ev.detail) {
          if (ev.detail.thinking) this._thinkMode = Number(ev.detail.thinking);
          if (ev.detail.speaking) this._speakMode = Number(ev.detail.speaking);
          if (ev.detail.listening) this._listenMode = Number(ev.detail.listening);
        }
      });
      if (!this.shadowRoot) {
        const root=this.attachShadow({mode:'open'});
        // Constructed stylesheet (adoptedStyleSheets), not a parsed <style>
        // tag: the stage window's CSP injects a per-load nonce into
        // style-src for Tauri's own IPC init script, which per the CSP
        // spec makes 'unsafe-inline' inert for the WHOLE directive —
        // silently dropping any inline <style> content the same way it
        // drops inline style="" attributes. CSSStyleSheet.replaceSync is a
        // programmatic API, not inline-style text parsing, so it isn't
        // subject to this restriction (live bug, 2026-10-04).
        try {
          const sheet=new CSSStyleSheet();
          sheet.replaceSync(`:host{display:block;position:relative;width:100%;height:100%;aspect-ratio:1;contain:layout paint;pointer-events:none}
canvas{display:block;position:absolute;inset:0;width:100%;height:100%;pointer-events:none}`);
          root.adoptedStyleSheets=[sheet];
        } catch(e) {
          // Older engines without adoptedStyleSheets: fall back to a plain
          // <style> tag (works fine under a permissive/no-nonce CSP).
          const style=document.createElement('style');
          style.textContent=`:host{display:block;position:relative;width:100%;height:100%;aspect-ratio:1;contain:layout paint;pointer-events:none}
canvas{display:block;position:absolute;inset:0;width:100%;height:100%;pointer-events:none}`;
          root.appendChild(style);
        }
        // appendChild, not `innerHTML +=`: the latter re-serializes the
        // whole shadow root (including the fallback <style> tag above)
        // back to a string and re-parses it, hitting the exact same
        // CSP block a second time.
        root.appendChild(document.createElement('canvas')).setAttribute('aria-hidden','true');
        root.appendChild(document.createElement('canvas')).setAttribute('aria-hidden','true');
        [this._halo,this._canvas]=this.shadowRoot.querySelectorAll('canvas');
        this._hctx=this._halo.getContext('2d');
        this._setupRenderer();
        if(!this.hasAttribute('role'))this.setAttribute('role','img');
      }
      this._motion=matchMedia('(prefers-reduced-motion: reduce)');
      this._weights=weightsFor(this.state);
      this._motion.addEventListener('change',this._sync);
      this._visibilityHandler = () => {
        if (document.hidden) {
          cancelAnimationFrame(this._frame);
          this._frame = 0;
          this._last = 0;
        } else {
          this._sync();
        }
      };
      document.addEventListener('visibilitychange', this._visibilityHandler);
      this._observer=new ResizeObserver(()=>this._resize());
      this._observer.observe(this);
      this._resize();this._sync();this._label();
    }
    disconnectedCallback() {
      cancelAnimationFrame(this._frame);this._frame=0;this._last=0;
      this._observer?.disconnect();
      this._motion?.removeEventListener('change',this._sync);
      if (this._visibilityHandler) {
        document.removeEventListener('visibilitychange', this._visibilityHandler);
      }
      if (this._gl) {
        try {
          this._gl.getExtension('WEBGL_lose_context')?.loseContext();
        } catch (_) {}
        this._gl = null;
      }
      this.disconnect();
    }
    get state() { return STATES.includes(this.getAttribute('state'))?this.getAttribute('state'):'idle'; }
    set state(value) { this.setAttribute('state',value); }
    get thinkMode() { return this._thinkMode; }
    set thinkMode(value) {
      this._thinkMode = Number(value) || 6;
      this.setAttribute('think-mode', String(this._thinkMode));
    }
    get speakMode() { return this._speakMode; }
    set speakMode(value) {
      this._speakMode = Number(value) || 2;
      this.setAttribute('speak-mode', String(this._speakMode));
    }
    get listenMode() { return this._listenMode; }
    set listenMode(value) {
      this._listenMode = Number(value) || 1;
      this.setAttribute('listen-mode', String(this._listenMode));
    }
    get particles() {
      const fixed=Number(this.getAttribute('particles'));
      const count=this.hasAttribute('particles')&&Number.isFinite(fixed)?Math.round(clamp(fixed,200,20000)):this._auto;
      return this._gl ? (count||5000) : Math.min(count||1000,1500);
    }
    get bands() { return this._bands.slice(); }
    get audioContext() { return this._audio?.context || null; }
    get renderer() { return this._gl?'webgl':'canvas2d'; }
    attributeChangedCallback(name) {
      // An external state flip mid-text (e.g. the React prop or the orb
      // leaving speaking) cancels the text phase — the override wins and
      // the weights lerp straight to the new state. setText's own
      // 'text' write passes through (guard !== 'text').
      if(name==='state' && this._textPhase && this.getAttribute('state')!=='text') {
        this._textPhase=null;this._textProg=0;this._textPts=null;this._textPending=null;
      }
      if(name==='state') {
        this._label();
        console.log(`[ANIM] Orb state transition -> ${this.state} (color: ${this.getAttribute('color') || '#ffffff'})`);
      }
      if(name==='think-mode') {
        const tm = Number(this.getAttribute('think-mode'));
        if (tm) this._thinkMode = tm;
      }
      if(name==='speak-mode') {
        const sm = Number(this.getAttribute('speak-mode'));
        if (sm) this._speakMode = sm;
      }
      if(name==='listen-mode') {
        const lm = Number(this.getAttribute('listen-mode'));
        if (lm) this._listenMode = lm;
      }
      if(name==='particles')this._count=0;
      if(name==='recording'||name==='color')this._sync();
      if(this._motion?.matches&&!this.hasAttribute('recording')) {
        this._weights=weightsFor(this.state);this._paint(0,this._weights,[0,0,0],0);
      }
    }
    _label() { this.setAttribute('aria-label',`Voice orb: ${this.state}`); }
    _setupRenderer() {
      const gl=this._canvas.getContext('webgl',{alpha:true,premultipliedAlpha:true,antialias:false,preserveDrawingBuffer:true,powerPreference:'high-performance'});
      try {
        if(!gl)throw new Error('WebGL unavailable');
        const shaders=[gl.VERTEX_SHADER,gl.FRAGMENT_SHADER].map((type,i)=>{
          const shader=gl.createShader(type);gl.shaderSource(shader,i?FS:VS);gl.compileShader(shader);
          if(!gl.getShaderParameter(shader,gl.COMPILE_STATUS))throw new Error(gl.getShaderInfoLog(shader));
          return shader;
        });
        const program=gl.createProgram();shaders.forEach(s=>gl.attachShader(program,s));gl.linkProgram(program);
        if(!gl.getProgramParameter(program,gl.LINK_STATUS))throw new Error(gl.getProgramInfoLog(program));
        shaders.forEach(s=>gl.deleteShader(s));
        gl.useProgram(program);this._buffer=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,this._buffer);
        const attr=gl.getAttribLocation(program,'seed');gl.enableVertexAttribArray(attr);gl.vertexAttribPointer(attr,4,gl.FLOAT,false,0,0);
        this._scatterLoc=gl.getAttribLocation(program,'scatter');
        this._scatterBuf=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,this._scatterBuf);
        if(this._scatterLoc>=0){gl.enableVertexAttribArray(this._scatterLoc);gl.vertexAttribPointer(this._scatterLoc,3,gl.FLOAT,false,0,0);}/* -1 = declared but unused, optimized out by the GLSL compiler */
        this._textLoc=gl.getAttribLocation(program,'textPos');
        this._textBuf=gl.createBuffer();gl.bindBuffer(gl.ARRAY_BUFFER,this._textBuf);
        if(this._textLoc>=0){gl.enableVertexAttribArray(this._textLoc);gl.vertexAttribPointer(this._textLoc,4,gl.FLOAT,false,0,0);}
        gl.enable(gl.BLEND);gl.blendFunc(gl.ONE,gl.ONE);gl.disable(gl.DEPTH_TEST);
        this._gl=gl;this._program=program;this._uniforms={};
        for(const n of ['time','pixels','density','weights','bands','onset','assemble','tw','textProg','thinkIn','textTint','userTint','glitch','ink','u_think_mode','u_speak_mode','u_listen_mode'])this._uniforms[n]=gl.getUniformLocation(program,n);
        this._auto=matchMedia('(pointer: coarse)').matches?5040:6240;
        if(!this._lossHandler) {
          this._lossHandler=e=>{e.preventDefault();this._lost=true;this._sync();};
          this._restoreHandler=()=>{this._lost=false;this._count=0;this._setupRenderer();this._resize();this._sync();};
          this._canvas.addEventListener('webglcontextlost',this._lossHandler);
          this._canvas.addEventListener('webglcontextrestored',this._restoreHandler);
        }
        console.log('[ANIM] WebGL Context initialized: 6240 particles | Vendor: ' + gl.getParameter(gl.VENDOR) + ' | Renderer: ' + gl.getParameter(gl.RENDERER));
      } catch(error) {
        console.error('[ANIM] WebGL shader/context compilation error:', error);
        this._gl=null;this._auto=1000;
        // A canvas cannot switch context types after WebGL initialization.
        const replacement=document.createElement('canvas');replacement.setAttribute('aria-hidden','true');
        this._canvas.replaceWith(replacement);this._canvas=replacement;this._ctx=replacement.getContext('2d');
      }
      this._count=0;
    }
    pause() {
      this._paused = true;
      cancelAnimationFrame(this._frame);
      this._frame = 0;
      this._last = 0;
    }
    play() {
      this._paused = false;
      this._resize();
      this._sync();
    }
    // Intact sphere: instant readiness without scattered assembly flight
    assemble() {
      this._assemble = 1;
      this._assembling = false;
      if (!this._frame) this._sync();
    }
    disperse() {
      this._assemble = 1;
      this._assembling = false;
    }
    // ─── Particle-generated text (Feature 88) ─────────────────────
    // The SAME cloud particles detach, converge into readable glyphs,
    // hold, then dissolve back into the previous state. No overlay text,
    // no crossfade — one continuous particle simulation.
    // Returns false when the text renders empty (no-canvas graceful path).
    setText(_text, _holdMs = 400) {
      return false;
    }
    _resize() {
      const rect = this.getBoundingClientRect();
      const cssW = this.clientWidth || rect.width || 180;
      const size = Math.max(80, Math.round(cssW * Math.min(devicePixelRatio || 1, 2)));
      if(this._canvas.width!==size||this._canvas.height!==size) {
        this._canvas.width=this._canvas.height=this._halo.width=this._halo.height=size;
      }
      if(this.hasAttribute('recording')&&this._snapshot) {
        const f=this._snapshot;this._paint(f.time,f.weights,f.bands,f.onset);
      } else this._paint(this._motion?.matches?0:this._time,this._weights,this._motion?.matches?[0,0,0]:this._bands,this._onset);
    }
    _sync() {
      cancelAnimationFrame(this._frame);this._frame=0;this._last=0;
      if(!this.isConnected||this.hasAttribute('recording')||this._paused||this._lost||document.hidden)return;
      if(this._motion?.matches) { this._weights=weightsFor(this.state);this._paint(0,this._weights,[0,0,0],0);return; }
      this._frame=requestAnimationFrame(this._tick);
    }
    _tick(now) {
      if(this._paused || !this.isConnected || document.hidden) return;
      // Adaptive Frame Pacing: idle breathing is ~0.33Hz, so 25 FPS pacing
      // cuts GPU power by >60% while staying visually smooth. Active states
      // (listening, thinking, speaking) run at full monitor refresh rate (60+ FPS).
      if(this.state === 'idle') {
        const since = now - (this._lastPaint || 0);
        if(since < 40) {
          this._frame = requestAnimationFrame(this._tick);
          return;
        }
      }
      this._lastPaint = now;
      const elapsed=this._last?(now-this._last)/1000:1/60;
      const dt=Math.min(.08,elapsed);this._last=now;this._time+=dt;
      if(!this.hasAttribute('particles')) {
        this._slow=elapsed>.026?this._slow+1:Math.max(0,this._slow-1);
        if(this._slow>=45&&this._auto>800) {this._auto=Math.max(800,Math.round(this._auto*.7));this._slow=0;}
      }
      const k=1-Math.exp(-dt/.24),target=weightsFor(this.state);
      for(let i=0;i<5;i++)this._weights[i]+=(target[i]-this._weights[i])*k;
      if(this._assembling) {
        const p=Math.min(1,(now-this._assembleStart)/900);
        const e=p<0.5?4*p*p*p:1-Math.pow(-2*p+2,3)/2;
        this._assemble=this._assembleFrom+(this._assembleTarget-this._assembleFrom)*e;
        if(p>=1){this._assembling=false;this._assemble=this._assembleTarget;}
      }
      // Thinking entry: contract-then-expand (sphere pulls inward, then the
      // knot stretches out over ~700ms). Resets when the state leaves.
      if(this.state==='thinking'){if(!this._thinkStart)this._thinkStart=now;}
      else if(this._thinkStart)this._thinkStart=0;
      const tp2=this._thinkStart?Math.min(1,(now-this._thinkStart)/700):1;
      this._thinkIn=tp2*tp2*(3-2*tp2);
      // Glitch transition: fire a short (~220ms) burst whenever the
      // dominant base-state weight flips (naturally lands mid-blend, when
      // the new state's weight crosses the old one — not at the instant
      // the state attribute changes). Suspended during particle-text
      // (weights[0..3] all decay toward 0 there, which would otherwise
      // Glitch disabled: state morphs transition smoothly and cleanly
      this._glitchAmt = 0;
      // Particle-text phase machine: converge (~700ms) → hold → dissolve
      // (~600ms, state reverts so the sphere reforms while particles
      // stream back with a swirl burst) → cleanup.
      if(this._textPhase==='converge') {
        const p=Math.min(1,(now-this._textStart)/700);
        this._textProg=p<0.5?4*p*p*p:1-Math.pow(-2*p+2,3)/2;
        if(p>=1){this._textPhase='hold';this._textStart=now;}
      } else if(this._textPhase==='hold') {
        this._textProg=1;
        if(now-this._textStart>=this._textHold) {
          this._textPhase='dissolve';this._textStart=now;
          if(this.state==='text'){this.state=this._prevState;this._label();}
        }
      } else if(this._textPhase==='dissolve') {
        const p=Math.min(1,(now-this._textStart)/600);
        this._textProg=1-p;
        if(p>=1){this._textPhase=null;this._textProg=0;this._textPts=null;this._textPending=null;}
      }
      this._readAudio(dt);
      this._paint(this._time,this._weights,this._bands,this._onset);
      this._frame=requestAnimationFrame(this._tick);
    }
    renderAt(time, {weights=weightsFor(this.state),bands=[0,0,0],onset=0}={}) {
      const frame={time:Number.isFinite(+time)?+time:0,weights:normalize(weights),bands:[0,1,2].map(i=>clamp(bands[i])),onset:clamp(onset)};
      this._snapshot=frame;
      if(this._hctx){this._paint(frame.time,frame.weights,frame.bands,frame.onset);this._gl?.flush();}
    }
    _paint(time,weights,bands,onset) {
      if(this._lost)return;
      const size=this._canvas.width,count=this.particles;
      if(count!==this._count||this._textPending) {
        this._count=count;this._seeds=sphere(count);this._scat=scatterOf(count);
        this._textPosArr=textOf(count,this._textPending||this._textPts);
        this._textPending=null;
        if(this._gl){
          const gl=this._gl;
          gl.bindBuffer(gl.ARRAY_BUFFER,this._buffer);gl.bufferData(gl.ARRAY_BUFFER,this._seeds,gl.STATIC_DRAW);
          gl.bindBuffer(gl.ARRAY_BUFFER,this._scatterBuf);gl.bufferData(gl.ARRAY_BUFFER,this._scat,gl.STATIC_DRAW);
          gl.bindBuffer(gl.ARRAY_BUFFER,this._textBuf);gl.bufferData(gl.ARRAY_BUFFER,this._textPosArr,gl.STATIC_DRAW);
        }
      }
      const uColor=this.userColor;
      PALETTE[0]=uColor;
      PALETTE[1]=uColor;
      const rgb=[0,1,2].map(c=>Math.round(PALETTE.reduce((sum,p,i)=>sum+p[c]*weights[i],0)*255));
      // Text inherits the STATE palette (Feature 89): the dominant base
      // state's color becomes the textTint — glyphs formed during
      // listening are gold/white, thinking purple/blue, speaking
      // magenta/white. No separate text color exists.
      const w4=weights[4]||0;
      const inv=w4<0.999?1/(1-w4):1;
      const wMain=weights.slice(0,4).map(v=>v*inv);
      let dom=0,domV=-1;
      for(let i=0;i<4;i++){const v=wMain[i];if(v>domV){domV=v;dom=i;}}
      const stateTint=PALETTE[dom];
      const tintRgb=[0,1,2].map(c=>Math.round((PALETTE.reduce((sum,p,i)=>i<4?sum+p[c]*weights[i]:sum,0)*inv+stateTint[c]*w4)*255));
      const h=this._hctx;h.clearRect(0,0,size,size);
      const glow=h.createRadialGradient(size*.5,size*.5,size*.10,size*.5,size*.5,size*.48);
      const energy=(weights[1]+weights[3])*(bands[0]*.025+onset*.035);
      // Ramp the glow in WITH the particle entrance (this._assemble, the
      // same 0..1 progress the vertex shader's `assemble` uniform already
      // uses to fly particles in from scatter) instead of painting it at
      // full strength from frame 1 — previously the glow appeared instantly
      // on every wake while the particles were still mid-flight, reading as
      // something rendering at the orb's position before the sphere itself
      // (live bug, 2026-10-04, found via user report).
      const glowIn=this._assemble==null?1:this._assemble;
      glow.addColorStop(0,`rgba(${tintRgb},${.012*glowIn})`);glow.addColorStop(.58,`rgba(${tintRgb},${(.028+energy)*glowIn})`);glow.addColorStop(1,`rgba(${tintRgb},0)`);
      h.fillStyle=glow;h.fillRect(0,0,size,size);
      if(this._gl) {
        const gl=this._gl,u=this._uniforms;gl.viewport(0,0,size,size);gl.clearColor(0,0,0,0);gl.clear(gl.COLOR_BUFFER_BIT);
        gl.useProgram(this._program);gl.uniform1f(u.time,time);gl.uniform1f(u.pixels,size);
        gl.uniform1f(u.density,Math.pow(5000/count,.32));        gl.uniform4fv(u.weights,wMain);gl.uniform3fv(u.bands,bands);gl.uniform1f(u.onset,onset);
        gl.uniform1f(u.assemble,this._assemble);
        gl.uniform1f(u.tw,w4);
        gl.uniform1f(u.textProg,this._textProg||0);
        gl.uniform1f(u.thinkIn,this._thinkIn==null?1:this._thinkIn);
        gl.uniform3fv(u.textTint,stateTint);
        gl.uniform3fv(u.userTint,uColor);
        gl.uniform1f(u.glitch,this._glitchAmt||0);
        gl.uniform1i(u.u_think_mode,this._thinkMode||6);
        gl.uniform1i(u.u_speak_mode,this._speakMode||2);
        gl.uniform1i(u.u_listen_mode,this._listenMode||1);
        {const ink=this.getAttribute('theme')==='light';gl.uniform1f(u.ink,ink?1:0);gl.blendFunc(gl.ONE,ink?gl.ONE_MINUS_SRC_ALPHA:gl.ONE);}
        gl.drawArrays(gl.POINTS,0,count);
      } else this._paint2D(time,wMain,bands,onset,tintRgb,size,w4,this._textProg||0,this._thinkIn==null?1:this._thinkIn,this._textPosArr,this._glitchAmt||0,uColor);
    }
    _paint2D(t,w,b,onset,rgb,size,tw=0,textProg=0,thinkIn=1,textArr=null,glitchAmt=0,userTint=[.95,.72,.35]) {
      const ctx=this._ctx;const ink=this.getAttribute('theme')==='light';ctx.clearRect(0,0,size,size);ctx.globalCompositeOperation=ink?'source-over':'lighter';
      const baseFill=`rgb(${rgb})`;
      for(let i=0;i<this._count;i++) {
        const o=i*4,n=this._seeds,x=n[o],y=n[o+1],z=n[o+2],u=n[o+3];
        const a=t*(.14*(w[0]+w[1])+.35*w[3]);
        const c=Math.cos(a),sn=Math.sin(a),angle=Math.acos(clamp(z,-1,1));
        const isFixed=(i%2===0);
        let listenR=1.0;
        if(!isFixed){
          const soundEnergy=Math.max(b[0],Math.max(b[1],b[2]))*0.75+onset*0.85;
          const voiceLevel=clamp((soundEnergy-0.02)/0.98);
          if(voiceLevel>0.001){
            const h1=seeded(i*43+71);
            const h2=seeded(i*89+19);
            const h3=seeded(i*131+37);
            const bounce=Math.sin(t*(18.0+34.0*h1)+h2*6.2831853);
            const beatKick=(h1-0.5)*2.0*onset*0.45;
            const jitter=(h3-0.5)*0.16*b[1];
            listenR=1.0+voiceLevel*(0.35*bounce+beatKick+jitter);
          }
        }
        let r=listenR;
        let px=(x*c+z*sn)*r,pz=(z*c-x*sn)*r,py=y*r;
        let speakFace=0,speakBump=0;
        if(w[3]>.001) {
          // Dense irregular potato/pebble blob (parity with the GL path):
          // a dominant low-frequency noise octave produces fewer, bigger,
          // more irregular lobes; a smaller secondary octave plus
          // high-frequency grain add surface sparkle.
          const lobeNoise=vnoise(x*0.65+t*.22,y*0.65-t*.17,z*0.65+t*.13);
          const fluidNoise1 = vnoise(x*0.92+t*0.20, y*0.92-t*0.15, z*0.92+t*0.12);
          const fluidNoise2 = vnoise(x*1.75-t*0.10, y*1.75+t*0.16, z*1.75-t*0.08);
          const textDamp = 1.0 - 0.75 * textProg;
          const speechBulge = (0.14 * Math.max(b[0], b[1]) + 0.28 * onset) * textDamp;
          const dropSag = -0.11 * Math.max(0, -y) * (1.0 - 0.4 * x * x);
          const rad = 1.0 + 0.16 * fluidNoise1 + 0.07 * fluidNoise2 + speechBulge * fluidNoise1 + dropSag;
          const rot = t * 0.16;
          const rc = Math.cos(rot), rs = Math.sin(rot);
          const sx = x * rad, sy = y * rad, sz = z * rad;
          const bx = sx * rc + sz * rs, bz = -sx * rs + sz * rc, by = sy;
          speakFace = z;
          speakBump = 0.5 + 0.5 * fluidNoise1;
          px = px * (1 - w[3]) + bx * w[3];
          py = py * (1 - w[3]) + by * w[3];
          pz = pz * (1 - w[3]) + bz * w[3];
        }
        if(w[2] > 0.001) {
          let bx = 0, by = 0, bz = 0;
          if ((this._thinkMode || 6) === 6) {
            // #6 Trefoil Celtic Helix Cage (Default & User selection):
            const tStrand = Math.floor(u * 3.0);
            const tAng = (u * 3.0 % 1.0) * 6.2831853;
            const tPhase = tStrand * 2.0943951;
            const tR = 0.95 + 0.15 * Math.cos(3.0 * tAng + t * 1.2);
            let rx = tR * Math.cos(tAng + t * 0.5);
            let ry = tR * Math.sin(tAng + t * 0.5);
            let rz = 0.35 * Math.sin(2.0 * tAng + tPhase);
            if (tStrand === 1) {
              const cy1 = Math.cos(1.05), sy1 = Math.sin(1.05);
              const ny = ry * cy1 - rz * sy1;
              const nz = ry * sy1 + rz * cy1;
              ry = ny; rz = nz;
            } else if (tStrand === 2) {
              const cx1 = Math.cos(1.05), sx1 = Math.sin(1.05);
              const nx = rx * cx1 - ry * sx1;
              const ny = rx * sx1 + ry * cx1;
              rx = nx; ry = ny;
            }
            const aRot = t * 0.35;
            const ca = Math.cos(aRot), sa = Math.sin(aRot);
            const txRot = ca * (rx + x * 0.06) + sa * (rz + z * 0.06);
            const tyRot = ry + y * 0.06;
            const tzRot = ca * (rz + z * 0.06) - sa * (rx + x * 0.06);
            const scale = mixNum(0.70, 1.0, thinkIn);
            bx = txRot * scale;
            by = tyRot * scale;
            bz = tzRot * scale;
          } else {
            // 3D woven luminous violet ribbon knot:
            const ku = u * 6.2831853;
            const kv = (y * 0.5 + 0.5) * 2.0 - 1.0;
            const kw = (z * 0.5 + 0.5) * 2.0 - 1.0;
            const knotR = 0.88 * (1.0 + 0.38 * Math.cos(3.0 * ku + t * 0.40));
            const kcx = knotR * Math.cos(2.0 * ku + t * 0.26);
            const kcy = knotR * Math.sin(2.0 * ku + t * 0.26);
            const kcz = -0.54 * Math.sin(3.0 * ku + t * 0.40);

            const kuNext = ku + 0.015;
            const knotRNext = 0.88 * (1.0 + 0.38 * Math.cos(3.0 * kuNext + t * 0.40));
            const knx = knotRNext * Math.cos(2.0 * kuNext + t * 0.26);
            const kny = knotRNext * Math.sin(2.0 * kuNext + t * 0.26);
            const knz = -0.54 * Math.sin(3.0 * kuNext + t * 0.40);

            const [tx, ty, tz] = normalize3(knx - kcx, kny - kcy, knz - kcz);
            const [p1x, p1y, p1z] = normalize3(...cross3(tx, ty, tz, 0, 0, 1));
            const [p2x, p2y, p2z] = cross3(tx, ty, tz, p1x, p1y, p1z);

            const ribbonWidth = 0.26 * (0.85 + 0.15 * Math.sin(ku * 4.0 + t));
            const ribbonThick = 0.04;
            const sx0 = kcx + p1x * (kv * ribbonWidth) + p2x * (kw * ribbonThick);
            const sy0 = kcy + p1y * (kv * ribbonWidth) + p2y * (kw * ribbonThick);
            const sz0 = kcz + p1z * (kv * ribbonWidth) + p2z * (kw * ribbonThick);

            const yaw = t * 0.42, cy = Math.cos(yaw), sy_rot = Math.sin(yaw);
            const pitch = 0.12 * Math.sin(t * 0.30), cp = Math.cos(pitch), sp_rot = Math.sin(pitch);
            const x1 = cy * sx0 + sy_rot * sz0;
            const y1 = sy0;
            const z1 = cy * sz0 - sy_rot * sx0;
            const tx0 = x1;
            const ty0 = cp * y1 - sp_rot * z1;
            const tz0 = cp * z1 + sp_rot * y1;
            const scale = mixNum(0.70, 1.0, thinkIn);
            bx = tx0 * scale;
            by = ty0 * scale;
            bz = tz0 * scale;
          }
          px = px * (1 - w[2]) + bx * w[2];
          py = py * (1 - w[2]) + by * w[2];
          pz = pz * (1 - w[2]) + bz * w[2];
        }
        let ambF=0;
        if(tw>0.001&&textArr) {
          // Particle text sandfall stream from south pole (parity with GL path):
          const ti=i*4;
          const isG=textArr[ti+3]<0.5;
          const tst=seeded(i+293);
          const tp0=clamp((textProg-tst*0.25)/0.75);
          const te2=tp0*tp0*(3-2*tp0);
          const spX=x*0.18, spY=-1.02+y*0.06, spZ=z*0.18;
          const fallArcY=-0.28*Math.sin(te2*Math.PI);
          const turbX=Math.sin(t*3.2+tst*6.28)*0.12*(1-te2);
          const turbY=Math.cos(t*2.6+tst*6.28)*0.08*(1-te2);
          const tx=isG?(mixNum(spX,textArr[ti],te2)+turbX):textArr[ti];
          const ty2=isG?(mixNum(spY,textArr[ti+1],te2)+fallArcY+turbY):textArr[ti+1];
          const tz2=isG?mixNum(spZ,textArr[ti+2],te2):textArr[ti+2];
          px=px*(1-tw)+tx*tw;
          py=py*(1-tw)+ty2*tw;
          pz=pz*(1-tw)+tz2*tw;
          ambF=(isG?0:1)*clamp(tw*1.6);
        }
        const perspective=3.8/(3.8-pz*.6),rim=Math.pow(1-Math.abs(z),2);
        // Dot radius parity with the GL path's point-size fix: scaled by 0.60 (-40%)
        const dot=Math.max(0.8,size/480*(0.66+.24*(pz+1)+rim*.48)*(1-.28*w[3])*mixNum(1.0,.62,ambF));
        ctx.globalAlpha=clamp((.20+.24*(pz+1)+rim*.3+w[2]*.4+w[3]*(speakFace*.22+speakBump*.15)+tw*(.30+.25*(pz+1)))*mixNum(1.0,.32,ambF));
        const stateColor=[1.0, 1.0, 1.0];
        ctx.fillStyle=ink?`rgb(0,0,0)`:`rgb(255,255,255)`;
        ctx.beginPath();ctx.arc(size*(.5+px*perspective*.305),size*(.5-py*perspective*.305),dot,0,Math.PI*2);ctx.fill();
      }
      ctx.globalAlpha=1;ctx.globalCompositeOperation='source-over';
    }

    async connect(source) {
      const isStream=typeof MediaStream!=='undefined'&&source instanceof MediaStream;
      const isMedia=typeof HTMLMediaElement!=='undefined'&&source instanceof HTMLMediaElement;
      const isNode=source&&typeof source.connect==='function'&&source.context&&typeof source.context.createAnalyser==='function';
      if(!isStream&&!isMedia&&!isNode)throw new TypeError('connect() needs a MediaStream, HTMLMediaElement or AudioNode.');
      const AC=window.AudioContext||window.webkitAudioContext;
      if(!AC)throw new Error('Web Audio is unavailable in this browser.');
      this.disconnect();
      let context,node,owned=false;
      if(isNode) {context=source.context;node=source;}
      else if(isMedia) {
        let entry=mediaSources.get(source);
        if(!entry) {
          context=new AC();
          try{node=context.createMediaElementSource(source);}catch(e){await context.close();throw e;}
          // Keep normal playback when the analysis tap is detached or the element is removed.
          node.connect(context.destination);entry={context,node};mediaSources.set(source,entry);
        }
        ({context,node}=entry);
      } else {context=new AC();owned=true;node=context.createMediaStreamSource(source);}
      const analyser=context.createAnalyser();analyser.fftSize=2048;analyser.smoothingTimeConstant=0;
      analyser.minDecibels=-85;analyser.maxDecibels=-10;
      // A silent output keeps stream analysis running without monitoring the microphone.
      const mute=context.createGain();mute.gain.value=0;analyser.connect(mute);mute.connect(context.destination);
      node.connect(analyser);
      const audio={context,node,analyser,mute,owned,data:new Float32Array(analyser.frequencyBinCount)};
      this._audio=audio;
      try {if(context.state!=='running')await context.resume();}
      catch(e){if(this._audio===audio)this.disconnect();throw e;}
      return this;
    }
    disconnect() {
      const a=this._audio;this._audio=null;
      if(a) {
        try{a.node.disconnect(a.analyser);}catch(_){}
        a.analyser.disconnect();a.mute.disconnect();
        if(a.owned&&a.context.state!=='closed')a.context.close().catch(()=>{});
      }
      this._bands=[0,0,0];this._onset=0;this._bassHistory=0;
    }
    setLevel(vol) {
      const v = clamp(vol);
      this._manualRaw = [v * 1.0, v * 0.85, v * 0.70];
    }
    _readAudio(dt) {
      const raw=[0,0,0],a=this._audio;
      if(a?.context.state==='running') {
        a.analyser.getFloatFrequencyData(a.data);
        const limits=[[45,250],[250,2400],[2400,12000]],step=a.context.sampleRate/a.analyser.fftSize;
        for(let k=0;k<3;k++) {
          const lo=Math.max(1,Math.ceil(limits[k][0]/step)),hi=Math.min(a.data.length-1,Math.floor(limits[k][1]/step));
          let power=0;
          for(let j=lo;j<=hi;j++)power+=Math.pow(10,a.data[j]/10);
          // Sum power so a narrow bass note is not diluted by unused FFT bins.
          const amplitude=Math.sqrt(power);
          raw[k]=clamp((Math.sqrt(amplitude)*[2.0,2.15,2.5][k]-.025)/.975);
        }
      } else if(this._manualRaw) {
        raw[0]=this._manualRaw[0];
        raw[1]=this._manualRaw[1];
        raw[2]=this._manualRaw[2];
        this._manualRaw=null;
      }
      // A fast baseline cancels a rolling 808 line, so the kick on top of it still reads as a hit.
      const flux=Math.max(0,raw[0]-this._bassHistory-.04)*6;
      this._bassHistory+=(raw[0]-this._bassHistory)*(1-Math.exp(-dt/.06));
      this._onset=Math.max(clamp(flux),this._onset*Math.exp(-dt/.18));
      const attack=[.012,.028,.006],release=[.20,.15,.085];
      for(let k=0;k<3;k++)this._bands[k]+=(raw[k]-this._bands[k])*(1-Math.exp(-dt/(raw[k]>this._bands[k]?attack[k]:release[k])));
    }
  }
  if (typeof customElements !== 'undefined') {
    if (!customElements.get('voice-orb')) {
      customElements.define('voice-orb',VoiceOrb);
    }
  }
})();
