import { phiGrid, underlaySpiral } from "./golden";
import "./design1118.css";

const W = 1618, H = 1000;
const GRID = phiGrid(W, H);
const SPIRAL = underlaySpiral(W, H);

/** design-1118a round 6: the faint golden spiral and φ grid behind a page (1110g). Decorative only. */
export function GoldenUnderlay() {
  return (
    <svg className="d18-underlay" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="xMidYMin meet" aria-hidden="true" focusable="false" data-testid="golden-underlay">
      <path className="d18-underlay__grid" d={GRID} />
      <path className="d18-underlay__spiral" d={SPIRAL} />
    </svg>
  );
}
