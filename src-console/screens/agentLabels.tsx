/** "Hermes Agent" with the model only in the native tooltip (long model ids truncate in 260px rows). */
export function HermesSubtitle({ model }: { model?: string | null }) {
  return <span title={model || undefined}>Hermes Agent</span>;
}
