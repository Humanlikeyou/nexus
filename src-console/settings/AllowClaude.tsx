import { useEffect, useState } from "react";
import { getAllowClaude, setAllowClaude } from "../bridge";
import { InsetGroup, ListRow, Toggle } from "../ui";

export const ALLOW_CLAUDE_COPY = {
  subtitle: "Lets Amara use Claude for some requests. This sends data off the Mac.",
  footer: "Off by default. Amara uses the local models unless this is on, a Claude key is set and outbound sends aren't disabled.",
  error: "Couldn't save this setting. Try again.",
};

/** Settings › Assistant: the opt-in for the Claude tier (llm_router). Off by default. */
export function AllowClaudeGroup() {
  const [allowed, setAllowed] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    getAllowClaude()
      .then((value) => { if (active) { setAllowed(value === true); setLoaded(true); } })
      .catch(() => { if (active) setLoaded(true); });
    return () => { active = false; };
  }, []);
  const change = async (next: boolean) => {
    setError(null);
    setAllowed(next);
    try {
      setAllowed((await setAllowClaude(next)) === true);
    } catch {
      setAllowed(!next);
      setError(ALLOW_CLAUDE_COPY.error);
    }
  };
  return (
    <InsetGroup title="Assistant" footer={error ?? ALLOW_CLAUDE_COPY.footer}>
      <ListRow title="Allow Claude" subtitle={ALLOW_CLAUDE_COPY.subtitle}
        accessory={<Toggle aria-label="Allow Claude" checked={allowed} disabled={!loaded} onCheckedChange={(v) => { void change(v); }} />} />
    </InsetGroup>
  );
}
