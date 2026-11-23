import type { ReactNode } from "react";
import { Button, DataTag } from "../ui";
import type { ContentState, HomeWindow } from "./types";

const TAG = <DataTag kind="sample" />;

export function SkeletonBody() {
  return <div className="w-skel" aria-hidden="true"><i style={{ width: "38%" }} /><i className="b" /><i style={{ width: "89%" }} /><i style={{ width: "61.8%" }} /></div>;
}

export function ChartBody() {
  const N = 48, W = 600, H = 300;
  let p = 60600;
  const bars: Array<[number, number, number, number]> = [];
  for (let i = 0; i < N; i++) {
    const d = Math.sin(i * 0.7) * 420 + Math.cos(i * 1.9) * 240 + (i > 24 ? 90 : -20);
    const o = p, cl = p + d;
    bars.push([o, cl, Math.max(o, cl) + 90 + (i % 3) * 60, Math.min(o, cl) - 80 - (i % 4) * 40]);
    p = cl;
  }
  const lo = Math.min(...bars.map((b) => b[3])) - 100, hi = Math.max(...bars.map((b) => b[2])) + 100;
  const y = (v: number) => H - ((v - lo) / (hi - lo)) * H;
  const hiBar = Math.max(...bars.slice(0, 20).map((b) => b[2]));
  const ax = [0, 1, 2, 3, 4].map((k) => Math.round((hi - (k * (hi - lo)) / 4) / 10) * 10);
  return (
    <div className="tv">
      <div className="tv__top"><b>BTCUSD</b><span>4h</span><span className="tv__up">63,140 ▲ 3.1%</span><span className="w-grow" />{TAG}</div>
      <div className="tv__plot">
        <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label="Sample candlestick chart, BTCUSD 4 hours">
          <rect x="0" width={W} y={y(hiBar)} height="1.5" fill="var(--copper-text)" opacity=".8" />
          {bars.map(([o, cl, h, l], i) => {
            const x = 6 + i * (W - 12) / N; const up = cl >= o;
            const color = up ? "var(--sys-green)" : "var(--sys-red)";
            return (
              <g key={i}>
                <line x1={x + 4} x2={x + 4} y1={y(h)} y2={y(l)} stroke={color} strokeWidth="1" vectorEffect="non-scaling-stroke" />
                <rect x={x} width="8" y={y(Math.max(o, cl))} height={Math.max(1.5, Math.abs(y(o) - y(cl)))} fill={color} />
              </g>
            );
          })}
        </svg>
        <div className="tv__axis" aria-hidden="true">{ax.map((v) => <span key={v}>{v.toLocaleString("en-US")}</span>)}</div>
      </div>
      <div className="tv__x" aria-hidden="true"><span>Mon</span><span>Tue</span><span>Wed</span><span>Thu</span><span>Fri</span></div>
    </div>
  );
}

export function VideoBody() {
  return (
    <div className="yt">
      <div className="yt__frame" role="img" aria-label="Sample video frame"><div className="yt__bar"><div className="yt__track"><i /></div><span className="yt__time">4:10 / 12:03</span></div></div>
      <div className="yt__meta">Label approval, step by step {TAG}<small>Sample channel · 18K views</small></div>
    </div>
  );
}

const TILES = ["#d8a06b,#8a4fd6", "#3f6e62,#9cc9b7", "#e2466f,#f5b04a", "#2d4a43,#6b4a2a", "#a7b8c9,#4a5b8c", "#f3d9b1,#c9734b", "#6c8c5a,#2d4a43", "#efe6d8,#b9a48a", "#4a3b5c,#e2466f"];
export function SocialBody() {
  return (
    <div className="ig">
      <div className="ig__stories" aria-label="Stories (sample)">{Array.from({ length: 7 }, (_, i) => <i key={i} />)}<span className="w-grow" />{TAG}</div>
      <div className="ig__grid" role="img" aria-label="Sample posts">{TILES.map((g) => <i key={g} style={{ background: `linear-gradient(135deg,${g})` }} />)}</div>
    </div>
  );
}

export function PdfBody() {
  return (
    <div className="pdf">
      <div className="pdf__top"><span>Page 1 of 6</span><span>Local file · read on this Mac</span><span className="w-grow" />{TAG}</div>
      <div className="pdf__page" aria-label="PDF page 1 (sample)">
        <h4>Residential Lease Renewal (sample)</h4>
        <p>This agreement renews the lease for Apartment 302 for a further term of two years beginning <mark>April 1, 2027</mark>.</p>
        <p>Either party who does not wish to renew must give written notice <mark>no later than one month</mark> before the end of the current term.</p>
        <p>A renewal fee equal to <mark>one month's rent</mark> is payable on renewal. Rent and common charges are unchanged.</p>
        <p>Early termination is covered in clause 9 on page 4.</p>
      </div>
    </div>
  );
}

export function WebBody() {
  return (
    <div className="web">
      <div className="web__url"><LockIcon /><span>example-news.jp/akiya-explained</span><span className="w-grow" />{TAG}</div>
      <div className="web__art">
        <div className="web__hero" role="img" aria-label="Sample article image" />
        <h3>Japan's akiya market, explained</h3>
        <p>About 9 million homes stand empty across Japan. Local "akiya banks" list many of them, often for very little, but repairs and taxes can add up quickly.</p>
        <p>Buyers usually need a local agent and a building survey before an offer.</p>
      </div>
    </div>
  );
}

function LockIcon() {
  return <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true"><rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" /></svg>;
}

export function SignInBody({ host }: { host: string }) {
  return (
    <div className="si">
      <div className="si__banner">
        <LockIcon />
        <span><b>{host}, inside Amara.</b> This is the site's own sign-in page in a private browser profile kept for this site on this Mac. What you type goes only to the site: Amara never reads it, and it never lands in chat or logs. You sign in once and stay signed in.</span>
      </div>
      <div className="si__form" aria-label={`${host} sign-in page (placeholder)`}>
        <h3>{host.replace(/\.\w+$/, "")}</h3>
        <p className="si__ph">The site's own form shows here. These fields are placeholders and collect nothing.</p>
        <label htmlFor="w-si-u">Phone, username or email (placeholder)</label>
        <input id="w-si-u" disabled placeholder="Placeholder · site's own field" autoComplete="off" />
        <label htmlFor="w-si-p">Password (placeholder)</label>
        <input id="w-si-p" type="password" disabled placeholder="Placeholder · site's own field" autoComplete="off" />
        <Button size="small" disabled>Log in (site's button)</Button>
      </div>
    </div>
  );
}

export function StateBody({ state, host, onSignIn, onPopOut, onRetry }: { state: ContentState; host: string; onSignIn?: () => void; onPopOut?: () => void; onRetry?: () => void }) {
  if (state === "loading" || state === "ready") return null;
  if (state === "signing_in") return <SignInBody host={host} />;
  const copy: Record<string, { h: string; p: string; act?: ReactNode }> = {
    signed_out: { h: `Sign in to ${host} once`, p: "Sign in inside Amara, on this site's own page. Amara reads nothing while you do. You stay signed in on this Mac.", act: <><Button size="small" onClick={onSignIn}>Sign in inside Amara</Button> <Button size="small" variant="secondary">Not now</Button></> },
    blocked: { h: "This site won't open inside Home", p: "It refuses to be embedded. Open it in its own window on this Mac instead.", act: <Button size="small" onClick={onPopOut}>Open in pop-out window</Button> },
    offline: { h: "You're offline", p: "The last time this page loaded is kept on this Mac. Try again when you're back online.", act: <Button size="small" onClick={onRetry}>Try again</Button> },
    unwired: { h: "Needs wiring", p: "Home windows need a webview from Claude before a live page can open here. Nothing on this card is made up.", act: <DataTag kind="wire" /> },
  };
  const c = copy[state];
  if (!c) return null;
  return <div className="w-center"><h3>{c.h}</h3><p>{c.p}</p>{c.act}</div>;
}

export function SampleBody({ win }: { win: HomeWindow }) {
  if (win.contentState === "loading") return <SkeletonBody />;
  if (win.contentState !== "ready") return null;
  switch (win.kind) {
    case "chart": return <ChartBody />;
    case "video": return <VideoBody />;
    case "social": return <SocialBody />;
    case "pdf": return <PdfBody />;
    default: return <WebBody />;
  }
}
