// design-1118b: `three` ships no TypeScript types and @types/three would be a second new dependency
// (the brief allows only `three`). The engine is typed at its own boundary instead.
declare module "three";
declare module "three/examples/jsm/misc/GPUComputationRenderer.js";
