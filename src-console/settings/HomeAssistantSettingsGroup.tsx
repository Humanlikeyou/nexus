import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import {
  COMING_SOON_COPY, HA_DEFAULT_URL, haConfigBackend, isCommandMissing, saveErrorCopy, testResultCopy, tokenError, urlError, type HaConfig,
} from "../lib/haConfig";
import { Badge, Button, InsetGroup, ListRow, TextField } from "../ui";

export const HA_SETTINGS_ANCHOR = "home-assistant";

type Load = { status: "loading" } | { status: "ready"; config: HaConfig } | { status: "coming_soon" } | { status: "error" };
type Result = { ok: boolean; text: string } | null;

/**
 * Settings › Home Assistant. The token is write-only: it lives in local state only while typed,
 * is cleared after save, and is never read back, prefilled, logged or put in an error.
 */
export function HomeAssistantSettingsGroup() {
  const [load, setLoad] = useState<Load>({ status: "loading" });
  const [url, setUrl] = useState("");
  const [urlTouched, setUrlTouched] = useState(false);
  const [token, setToken] = useState("");
  const [replacing, setReplacing] = useState(false);
  const [busy, setBusy] = useState<"save" | "clear" | "test" | null>(null);
  const [saveResult, setSaveResult] = useState<Result>(null);
  const [testResult, setTestResult] = useState<Result>(null);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);

  const refresh = useCallback(() => haConfigBackend().getConfig().then(
    (config) => {
      if (!mounted.current) return;
      setLoad({ status: "ready", config });
      setUrl(config.url ?? HA_DEFAULT_URL);
    },
    (err: unknown) => { if (mounted.current) setLoad(isCommandMissing(err) ? { status: "coming_soon" } : { status: "error" }); },
  ), []);
  useEffect(() => { void refresh(); }, [refresh]);

  const header = { title: "Home Assistant", footer: "Connects Smart home to Home Assistant on this Mac. The token is stored in your Keychain and never shown again." };

  if (load.status === "loading") {
    return <div id={HA_SETTINGS_ANCHOR}><InsetGroup {...header}><ListRow title="Status" value="Loading…" /></InsetGroup></div>;
  }
  if (load.status === "coming_soon") {
    return <div id={HA_SETTINGS_ANCHOR}><InsetGroup {...header}><ListRow title="Connection settings" value={COMING_SOON_COPY} /></InsetGroup></div>;
  }
  if (load.status === "error") {
    return (
      <div id={HA_SETTINGS_ANCHOR}>
        <InsetGroup {...header}>
          <ListRow title="Connection settings" subtitle="Couldn't load the Home Assistant settings."
            accessory={<Button size="small" variant="secondary" onClick={() => { setLoad({ status: "loading" }); void refresh(); }}>Try again</Button>} />
        </InsetGroup>
      </div>
    );
  }

  const { config } = load;
  const showToken = !config.tokenSet || replacing;
  const uErr = urlError(url);
  const tErr = tokenError(token);
  const urlChanged = url.trim() !== (config.url ?? "");
  const canSave = !uErr && !tErr && (urlChanged || token !== "") && busy === null;

  const fail = (err: unknown, missingSetter: () => void, fallback: (e: unknown) => string, set: (r: Result) => void) => {
    if (!mounted.current) return;
    if (isCommandMissing(err)) { missingSetter(); return; }
    set({ ok: false, text: fallback(err) });
  };
  const comingSoon = () => setLoad({ status: "coming_soon" });

  const onSave = async (e: FormEvent) => {
    e.preventDefault();
    setUrlTouched(true);
    if (!canSave) return;
    setBusy("save"); setSaveResult(null); setTestResult(null);
    const input: { url?: string; token?: string } = {};
    if (urlChanged) input.url = url.trim();
    if (token !== "") input.token = token;
    try {
      await haConfigBackend().setConfig(input);
      if (!mounted.current) return;
      setToken(""); setReplacing(false);
      setSaveResult({ ok: true, text: "Saved." });
      await refresh();
    } catch (err) {
      setToken("");
      fail(err, comingSoon, saveErrorCopy, setSaveResult);
    } finally { if (mounted.current) setBusy(null); }
  };

  const onClear = async () => {
    setBusy("clear"); setSaveResult(null); setTestResult(null);
    try {
      await haConfigBackend().clearToken();
      if (!mounted.current) return;
      setToken(""); setReplacing(false);
      setSaveResult({ ok: true, text: "Token cleared." });
      await refresh();
    } catch (err) {
      fail(err, comingSoon, () => "Couldn't clear the token. Try again.", setSaveResult);
    } finally { if (mounted.current) setBusy(null); }
  };

  const onTest = async () => {
    setBusy("test"); setTestResult(null);
    try {
      const result = await haConfigBackend().testConnection();
      if (mounted.current) setTestResult({ ok: result.ok, text: testResultCopy(result) });
    } catch (err) {
      fail(err, comingSoon, () => testResultCopy({ ok: false }), setTestResult);
    } finally { if (mounted.current) setBusy(null); }
  };

  return (
    <div id={HA_SETTINGS_ANCHOR}>
      <InsetGroup {...header}>
        <form className="settings__ha-form" onSubmit={(e) => { void onSave(e); }} noValidate aria-label="Home Assistant connection">
          <TextField label="Address" description="A local address on this Mac, like http://127.0.0.1:8123."
            value={url} inputMode="url" autoComplete="off" spellCheck={false}
            onChange={(e) => { setUrl(e.target.value); setSaveResult(null); }} onBlur={() => setUrlTouched(true)}
            error={urlTouched ? uErr ?? undefined : undefined} />
          <ListRow title="Access token"
            accessory={<Badge tone={config.tokenSet ? "success" : "neutral"}>{config.tokenSet ? "Saved" : "Not set"}</Badge>} />
          {showToken && (
            <TextField label={config.tokenSet ? "New access token" : "Access token"} type="password" autoComplete="off"
              spellCheck={false} name="ha-access-token" value={token}
              description="Create a long-lived access token in Home Assistant under Profile, then Security."
              onChange={(e) => { setToken(e.target.value); setSaveResult(null); }} error={tErr ?? undefined} />
          )}
          <div className="settings__ha-actions">
            <Button type="submit" variant="primary" size="small" loading={busy === "save"} disabled={!canSave}>Save</Button>
            {config.tokenSet && !replacing && (
              <Button type="button" size="small" variant="secondary" disabled={busy !== null} onClick={() => { setReplacing(true); setSaveResult(null); }}>Replace token</Button>
            )}
            {replacing && (
              <Button type="button" size="small" variant="secondary" disabled={busy !== null} onClick={() => { setReplacing(false); setToken(""); }}>Cancel</Button>
            )}
            {config.tokenSet && (
              <Button type="button" size="small" variant="secondary" loading={busy === "clear"} disabled={busy !== null} onClick={() => { void onClear(); }}>Clear token</Button>
            )}
            <Button type="button" size="small" variant="secondary" loading={busy === "test"} disabled={busy !== null} onClick={() => { void onTest(); }}>Test connection</Button>
          </div>
          <p className="settings__ha-result" role="status" aria-live="polite" data-ok={saveResult?.ok}>{saveResult?.text ?? ""}</p>
          <p className="settings__ha-result" role="status" aria-live="polite" data-ok={testResult?.ok}>{testResult?.text ?? ""}</p>
        </form>
      </InsetGroup>
    </div>
  );
}
