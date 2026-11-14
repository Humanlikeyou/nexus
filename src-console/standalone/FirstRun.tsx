import { useEffect, useRef, useState } from "react";
import { Button, DataTag, InsetGroup, ListRow, StatusText, Toggle } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import type { StandaloneAdapter } from "./adapters";
import { Bar, KeyState, Row, SaCard, Spin, WireNote } from "./parts";

const STEPS = ["Welcome", "Restore", "AI model", "Getting ready", "Preferences"] as const;

/**
 * First-run setup (design-1111s s1a–s1e): its own window before the shell. Opens on first launch once
 * setup_state is wired, and any time from Settings › General › Run Setup Again. Nothing here writes
 * anything until the backend exists; unwired parts say "Needs wiring".
 */
export function FirstRun({ adapter, onClose, again = false }: { adapter: StandaloneAdapter; onClose: () => void; again?: boolean }) {
  const [step, setStep] = useState(0);
  const [choice, setChoice] = useState<"restore" | "fresh">("restore");
  const [tier, setTier] = useState<string | null>(null);
  const [cloud, setCloud] = useState(false);
  const [login, setLogin] = useState(true);
  const [auto, setAuto] = useState(true);
  const head = useRef<HTMLHeadingElement>(null);
  useEffect(() => { head.current?.focus(); }, [step]);
  useEffect(() => {
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", esc);
    return () => window.removeEventListener("keydown", esc);
  }, [onClose]);
  const pw = adapter.profile(), p = valueOf(pw);
  const tw = adapter.tiers(), tiers = valueOf(tw);
  const dw = adapter.setupDownload(), dl = valueOf(dw);
  const rw = adapter.restorePreview(), rp = valueOf(rw);
  const keys = valueOf(adapter.secrets());
  const picked = tier ?? tiers?.find((t) => t.recommended)?.id ?? null;
  const n = step + 1;
  const next = () => setStep((s) => (s === 0 && choice === "fresh" ? 2 : Math.min(4, s + 1)));
  const back = () => setStep((s) => (s === 2 && choice === "fresh" ? 0 : Math.max(0, s - 1)));
  const downloading = !!dl && dl.models.some((m) => m.state !== "done");
  
  let body, foot;
  if (step === 0) {
    body = <>
      <h1 ref={head} tabIndex={-1}>Welcome to Amara</h1><p>Everything runs on this Mac: your assistant, memory, crews, health and automations. Nothing leaves it unless you turn that on.</p>
      <div className="pk-g2 sa-g2-even" role="radiogroup" aria-label="How to set up">
        <button type="button" role="radio" aria-checked={choice === "restore"} className="pk-choice" data-on={choice === "restore" || undefined} onClick={() => setChoice("restore")}>
          <span className="pk-ic" aria-hidden="true">↺</span><h3><span className="pk-radio" data-on={choice === "restore" || undefined} />Restore from a backup</h3>
          <p>Moving from another Mac? Bring back your chats, memory, crews, automations, Muse sessions, health store and settings in one step.</p>
          <p className="pk-note">You&apos;ll re-enter your keys afterwards. Keys are never inside a backup; the n8n key comes from your key file.</p></button>
        <button type="button" role="radio" aria-checked={choice === "fresh"} className="pk-choice" data-on={choice === "fresh" || undefined} onClick={() => setChoice("fresh")}>
          <span className="pk-ic" aria-hidden="true">✦</span><h3><span className="pk-radio" data-on={choice === "fresh" || undefined} />Start fresh</h3>
          <p>Set Amara up as new. You can still restore a backup later from Settings › Backup.</p></button>
      </div>
      <div className="pk-info">Setup checks this Mac&apos;s memory to choose the AI model size, downloads the models, and starts Amara&apos;s services. {p ? <>Expect about 20 minutes on a fast connection <DataTag kind="sample" /></> : <DataTag kind="wire" />}</div>
    </>;
    foot = <><span className="pk-note">FileVault: {p ? <><StatusText kind={p.fileVault ? "ok" : "risk"}>{p.fileVault ? "On" : "Off"}</StatusText> <DataTag kind="sample" /></> : <DataTag kind="wire" />}</span><Button size="small" variant="secondary" onClick={onClose}>{again ? "Close" : "Quit"}</Button><Button size="small" variant="primary" onClick={next}>Continue</Button></>;
  } else if (step === 1) {
    body = <>
      <h1 ref={head} tabIndex={-1}>Restore from a backup</h1><p>Amara checks the backup before touching anything. Nothing on this Mac is overwritten.</p>
      <div className="pk-g2">
        <div className="pk-stack">
          <SaCard title={rp?.file ?? "Backup file"} w={rw}>{rp ? <dl className="pk-kv"><dt>Made on</dt><dd>{rp.madeOn}</dd><dt>Amara version</dt><dd>{rp.version}</dd><dt>Size</dt><dd>{rp.size}</dd><dt>Checksum</dt><dd><StatusText kind={rp.checksumOk ? "ok" : "fail"}>{rp.checksumOk ? "Matches" : "Doesn't match"}</StatusText></dd></dl> : <WireNote>Needs wiring: pick a backup file and Amara checks it here (restore_preview).</WireNote>}</SaCard>
          {rp && <SaCard title="What comes back" w={rw}>{rp.contents.map((c) => <Row key={c.title} title={c.title} sub={c.sub}><StatusText kind="ok">Ready</StatusText></Row>)}</SaCard>}
        </div>
        <div className="pk-stack">
          <SaCard title="n8n key file">
            <Row title="n8n encryption key" sub="Opens saved n8n logins. Unlock it with your key file's passphrase.">{rp ? <StatusText kind={rp.keyFileFound ? "ok" : "off"}>{rp.keyFileFound ? "File found" : "Not found"}</StatusText> : <DataTag kind="wire" />}</Row>
            <div className="pk-actions"><Button size="small" variant="secondary" disabled={!rp}>Choose Key File…</Button><Button size="small" variant="primary" disabled={!rp?.keyFileFound}>Unlock…</Button></div>
            <p className="pk-note">The key file is kept beside your backups, never inside one. Without its passphrase n8n&apos;s saved logins can&apos;t be opened.</p>
          </SaCard>
          <SaCard title="You'll re-enter these">
            <p className="pk-note">Keys live in this Mac&apos;s Keychain only. Setup asks for each one after the restore.</p>
            {(keys ?? []).filter((k) => k.name !== "n8n_encryption_key").map((k) => <div key={k.name} className="pk-keyrow"><div>{k.label}{k.hint && <small>{k.hint}</small>}</div><KeyState set={false} /></div>)}
            {!keys && <WireNote>Needs wiring: the list of missing keys comes back from the restore.</WireNote>}
          </SaCard>
          <SaCard title="Then"><p className="pk-note">Re-pair your WHOOP 4 in Settings › Devices, and pick the model size for this Mac on the next step.</p></SaCard>
        </div>
      </div>
    </>;
    foot = <><span className="pk-note">Your old Mac keeps its copy. Restore is copy-and-verify.</span><Button size="small" variant="secondary" onClick={back}>Back</Button><Button size="small" variant="primary" onClick={next}>Restore</Button></>;
  } else if (step === 2) {
    body = <>
      <h1 ref={head} tabIndex={-1}>Pick the AI model size</h1><p>Bigger models think better but need more memory. Amara picked the best fit for this Mac.</p>
      <div className="pk-ram">{p ? <><span>This Mac has</span><b>{p.ramGb} GB</b><span>of memory · {p.chip}</span><DataTag kind="sample" /><span className="pk-note sa-push">Detected by Amara</span></> : <><span>This Mac&apos;s memory</span><DataTag kind="wire" /></>}</div>
      {tiers ? <div className="pk-g3" role="radiogroup" aria-label="AI model size">{tiers.map((t) => (
        <button key={t.id} type="button" role="radio" aria-checked={picked === t.id} disabled={!t.available} className="pk-choice" data-on={picked === t.id || undefined} data-disabled={!t.available || undefined} onClick={() => setTier(t.id)}>
          <h3><span className="pk-radio" data-on={picked === t.id || undefined} />{t.name}{t.recommended ? " · recommended" : ""}</h3>
          <p>{t.available ? `For Macs with ${t.minRamGb} GB or more.` : `Needs ${t.minRamGb} GB or more.${p ? ` This Mac has ${p.ramGb} GB.` : ""}`}</p>
          <ul>{t.models.map((m) => <li key={m}>{m}</li>)}</ul>
          <p className="pk-note">{t.download}{t.note ? ` · ${t.note}` : ""}</p></button>))}</div>
        : <SaCard title="Model sizes" w={tw}><WireNote>Needs wiring: Compact (16 GB+), Standard (32 GB+) and Large (64 GB+) come from the signed model manifest.</WireNote></SaCard>}
      <div className="pk-info">Model names and sizes {tiers ? <>are placeholders <DataTag kind="sample" /></> : "come from the signed manifest"}. Your current light model&apos;s name still needs your confirmation before setup ships. Cloud AI isn&apos;t part of this choice; it stays off.</div>
    </>;
    foot = <><span className="pk-note">You can change this later in Settings › General.</span><Button size="small" variant="secondary" onClick={back}>Back</Button><Button size="small" variant="primary" onClick={() => { if (picked) void adapter.startDownload(picked).then((r) => (r.state === "unwired" ? r : adapter.startServices())); next(); }}>Download and Continue</Button></>;
  } else if (step === 3) {
    body = <>
      <h1 ref={head} tabIndex={-1}>Getting Amara ready</h1><p>Downloading the models and starting services. You can leave this open; it carries on if the Mac sleeps.</p>
      <div className="pk-g2">
        <div className="pk-stack"><SaCard title="Models" w={dw}>{dl ? <>{dl.models.map((m) => <div key={m.name} className="pk-dl"><div className="pk-dl__top"><span>{m.name}</span>{m.state === "done" ? <StatusText kind="ok">Done</StatusText> : m.state === "downloading" ? <Spin>Downloading</Spin> : <StatusText kind="wait">Waiting</StatusText>}<span>{m.detail}</span></div><Bar pct={m.pct} label={`${m.name} download`} /></div>)}
          <p className="pk-note">Saved in Amara&apos;s own folder on this Mac. Checked against a signed list before use.</p></> : <WireNote>Needs wiring: resumable, checksum-checked downloads (model_download).</WireNote>}</SaCard></div>
        <div className="pk-stack"><SaCard title="Services" w={dw}>{dl ? dl.services.map((s) => <Row key={s.name} title={s.name} sub={s.sub}>{s.state === "running" ? <StatusText kind="ok">Running</StatusText> : s.state === "starting" ? <Spin>Starting</Spin> : <StatusText kind="wait">Waiting</StatusText>}</Row>) : <WireNote>Needs wiring: services start in order (services_start_all).</WireNote>}</SaCard></div>
      </div>
    </>;
    foot = <><span className="pk-note">Step 4 of 5 · nothing is sent anywhere except the model download.</span><Button size="small" variant="secondary" disabled={!dl}>Pause Download</Button><Button size="small" variant="primary" disabled={downloading} onClick={next}>Continue</Button></>;
  } else {
    body = <>
      <h1 ref={head} tabIndex={-1}>A few choices</h1><p>Sensible defaults are already set. Change anything later in Settings.</p>
      <div className="pk-g2">
        <div className="pk-stack">
          <InsetGroup title="Privacy">
            <ListRow title="Cloud AI (Claude, MiniMax)" subtitle="Off. Amara thinks on this Mac only. Turn on later if you want a cloud fallback." accessory={<Toggle aria-label="Cloud AI" checked={cloud} onCheckedChange={setCloud} />} />
            <ListRow title="Move keys into the Keychain" subtitle="Copies keys from old setup files into this Mac's Keychain. Values are never shown." accessory={<Button size="small" variant="secondary" disabled>Review…</Button>} />
          </InsetGroup>
          <InsetGroup title="Startup">
            <ListRow title="Open Amara at login" subtitle="On. Starts quietly so health sync, crews and reminders keep running." accessory={<Toggle aria-label="Open at login" checked={login} onCheckedChange={setLogin} />} />
            <ListRow title="Install updates automatically" subtitle="On. Signed updates install when Amara is idle." accessory={<Toggle aria-label="Auto-update" checked={auto} onCheckedChange={setAuto} />} />
          </InsetGroup>
        </div>
        <div className="pk-stack">
          <SaCard title="Permissions" tag="wire">
            <Row title="Bluetooth" sub="Muse headband and WHOOP 4 strap"><Button size="small" variant="primary" disabled>Allow…</Button></Row>
            <Row title="Microphone" sub="Voice and spoken check-ins (text only is kept)"><Button size="small" variant="secondary" disabled>Allow…</Button></Row>
            <Row title="Notifications" sub="Reminders on this Mac"><Button size="small" variant="secondary" disabled>Allow…</Button></Row>
          </SaCard>
          <SaCard title="Checks" w={pw}>{p ? <><Row title="FileVault" sub="Your data is encrypted at rest"><StatusText kind="ok">On</StatusText></Row><Row title="Services" sub="Everything Amara needs"><StatusText kind="ok">All running</StatusText></Row><Row title="Models" sub="Fast and deep ready"><StatusText kind="ok">Ready</StatusText></Row></> : <WireNote>Needs wiring: FileVault, services and models are checked here.</WireNote>}</SaCard>
        </div>
      </div>
    </>;
    foot = <><span className="pk-note">macOS asks for each permission itself; Amara only explains why.</span><Button size="small" variant="secondary" onClick={back}>Back</Button><Button size="small" variant="primary" onClick={() => { if (picked) void adapter.finishSetup(picked); onClose(); }}>Open Amara</Button></>;
  }
  return (
    <div className="pk-fr" data-first-run="">
      <div className="pk-fr__win" role="dialog" aria-modal="true" aria-label="Set up Amara OS">
        <div className="pk-fr__bar"><span className="pk-fr__lights" aria-hidden="true"><i /><i /><i /></span><span className="pk-fr__mark" aria-hidden="true">AMARA</span>
          <span className="pk-fr__prog" role="img" aria-label={`Step ${n} of 5: ${STEPS[step]}`}>{[1, 2, 3, 4, 5].map((i) => <i key={i} data-on={i <= n || undefined} />)}<span aria-hidden="true">&nbsp; Step {n} of 5 · {STEPS[step]}</span></span></div>
        <div className="pk-fr__body">{body}</div>
        <div className="pk-fr__foot">{foot}</div>
      </div>
    </div>
  );
}
