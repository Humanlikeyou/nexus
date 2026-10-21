import { tone, humanize, number } from "../lib/format";

export function State({ value, label }: { value: string | boolean | null | undefined; label?: string }) {
  const display = label ?? (typeof value === "boolean" ? (value ? "Online" : "Offline") : humanize(String(value ?? "unknown")));
  return <span className={`field-state field-state--${tone(value)}`}><i />{display}</span>;
}

export function Trace({ values, labels, tint = "cyan", tall = false }: { values: number[]; labels?: string[]; tint?: "cyan" | "lime" | "amber" | "coral"; tall?: boolean }) {
  const source = values.length > 1 ? values : [0, 0];
  const min = Math.min(...source);
  const max = Math.max(...source);
  const range = max - min || 1;
  const width = 800;
  const height = tall ? 240 : 130;
  const pad = 14;
  const points = source.map((value, index) => `${(index / (source.length - 1)) * width},${height - pad - ((value - min) / range) * (height - pad * 2)}`).join(" ");
  return (
    <figure className={`field-trace field-trace--${tint} ${tall ? "is-tall" : ""}`}>
      <svg viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" role="img" aria-label="Live data trend">
        <path className="field-trace__grid" d={`M0 ${height * .25}H800M0 ${height * .5}H800M0 ${height * .75}H800M160 0V${height}M320 0V${height}M480 0V${height}M640 0V${height}`} />
        <polyline className="field-trace__line" points={points} />
        {source.map((value, index) => <circle key={index} cx={(index / (source.length - 1)) * width} cy={height - pad - ((value - min) / range) * (height - pad * 2)} r="2.5" />)}
      </svg>
      {labels && labels.length > 0 && <figcaption><span>{labels[0]}</span><span>{labels[Math.floor(labels.length / 2)]}</span><span>{labels[labels.length - 1]}</span></figcaption>}
    </figure>
  );
}
