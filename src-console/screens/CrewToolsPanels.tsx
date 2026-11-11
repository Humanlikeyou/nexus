import { ChevronLeft } from "lucide-react";
import { useState, type ReactNode } from "react";
import { Button, Card, DataTag, Dialog, PageHero, SegmentedControl, StatusText, Toggle, wiredTag } from "../ui";
import { valueOf, type Wired } from "../lib/sampleAdapters";
import { CONFIRM_TIMEOUT_S, DEFAULT_DURATION, DURATION_ACTION, DURATION_LABEL, REQUEST_EXPIRY_MIN, type AuditRow, type CrewToolGrant, type CrewToolsAdapter, type GrantDuration } from "../crews/toolGrant";
import { CREW_TOOL_OUTPUT_CAP, CREW_TOOL_TIMEOUT_S } from "../app/statusViews";
import "../ui/PanelKit.css";
import "./CrewTools.css";

const Code = ({ children }: { children: string }) => <code className="pk-code">{children}</code>;

/** One line for the Crews table's "Files and terminal" column. */
export function GrantCell({ grant }: { grant: Wired<CrewToolGrant> }) {
  const g = valueOf(grant);
  if (!g) return <><StatusText kind="off">Off</StatusText><small>read-only · <DataTag kind="wire" /></small></>;
  if (g.status === "on") return <><StatusText kind="ok">On</StatusText><small>{g.expiresAt ? `until ${g.expiresAt}` : "this run"} · {g.scopePath}</small></>;
  if (g.status === "pending") return <><StatusText kind="wait">Asked</StatusText><small>waiting for you</small></>;
  if (g.status === "applying") return <><StatusText kind="run">Applying…</StatusText><small>waiting for crew-service</small></>;
  if (g.status === "error") return <><StatusText kind="fail">Off</StatusText><small>couldn&apos;t turn on</small></>;
  return <><StatusText kind="off">Off</StatusText><small>read-only</small></>;
}

/** The request card: a crew asked. Approve opens the same confirmation as the switch; nothing else can grant. */
function RequestCard({ crew, g, onDeny, onReview }: { crew: string; g: CrewToolGrant; onDeny: () => void; onReview: () => void }) {
  const r = g.request!;
  return (
    <Card title="Crew approvals · 1" accessory={<span className="pk-acc"><DataTag kind="sample" /></span>}>
      <div className="pk-row">
        <StatusText kind="wait" />
        <div className="pk-row__main"><span>{crew} asks for files and terminal</span>
          <small>{r.role} · &ldquo;{r.task}&rdquo; · {DURATION_LABEL[r.duration]} · {r.scopePath}</small>
          <small>Risk: high · Asked because {r.reason} · expires {r.expiresAt}</small></div>
        <span className="pk-pill" data-tone="warn">High risk</span>
        <Button size="small" onClick={onDeny}>Deny</Button>
        <Button size="small" variant="primary" onClick={onReview}>Review…</Button>
      </div>
    </Card>
  );
}

function Capabilities({ on, scope }: { on: boolean; scope: string }) {
  const row = (title: ReactNode, sub: string, status: ReactNode) => <div className="pk-row"><div className="pk-row__main"><span>{title}</span><small>{sub}</small></div>{status}</div>;
  return (
    <>
      {row("kb_query, memory_query", `Read the knowledge base and memory · ${CREW_TOOL_TIMEOUT_S} s timeout · ${CREW_TOOL_OUTPUT_CAP}-char cap`, <StatusText kind="ok">Allowed</StatusText>)}
      {row("Read files", on ? `Inside ${scope} only` : "Needs the switch", on ? <StatusText kind="ok">Allowed</StatusText> : <StatusText kind="off">Off</StatusText>)}
      {row("Write files", on ? "Inside the folder · git checkpoint before each write" : "Needs the switch", on ? <StatusText kind="ok">Allowed</StatusText> : <StatusText kind="off">Off</StatusText>)}
      {row("Terminal commands", on ? "Inside the folder · 60 s timeout · output capped, secrets redacted" : "Needs the switch", on ? <StatusText kind="ok">Allowed</StatusText> : <StatusText kind="off">Off</StatusText>)}
      {row("Delete, overwrite outside the folder", "Asks you every time, even with the switch on", <StatusText kind="wait">Asks first</StatusText>)}
      {row(<>Outbound, spending, <Code>git push</Code>, sudo</>, "Never allowed for crews", <StatusText kind="fail">Blocked</StatusText>)}
    </>
  );
}

function Audit({ rows }: { rows: Wired<AuditRow[]> }) {
  const list = valueOf(rows);
  return (
    <Card title="Audit" accessory={<span className="pk-acc">{wiredTag(rows)}</span>}>
      {!list ? <p className="pk-note">Every change, file write and terminal command will be listed here once crew-service reports its audit log.</p> : (
        <ol className="pk-audit" aria-label="Audit trail">
          {list.map((r) => (
            <li key={r.id}><time>{r.at}</time>
              <span>{r.kind === "blocked" ? <StatusText kind="fail" /> : r.kind === "asked" ? <StatusText kind="wait" /> : null}{r.kind === "grant" ? <b>{r.text}</b> : r.text}{r.target && <> <Code>{r.target}</Code></>}</span>
              <span className="pk-dim">{r.actor}</span></li>
          ))}
        </ol>
      )}
      <p className="pk-note">Every change and every file or terminal action is logged with who, when and what. Command output and secrets are not stored here.</p>
    </Card>
  );
}

/** Crews › <crew>: permissions, what the crew can use, agents, approver and the audit trail (design-1110i I2–I6). */
export function CrewToolsPage({ crew, agents, lastRun, adapter, onBack }: { crew: string; agents: string[]; lastRun: string; adapter: CrewToolsAdapter; onBack: () => void }) {
  const [, bump] = useState(0);
  const [sheet, setSheet] = useState<null | { via: "switch" | "request" }>(null);
  const [duration, setDuration] = useState<GrantDuration>(DEFAULT_DURATION);
  const [error, setError] = useState<string | null>(null);
  const grant = adapter.grant(crew);
  const g = valueOf(grant);
  const refresh = () => bump((n) => n + 1);
  const run = async (p: Promise<Wired<CrewToolGrant>>) => {
    refresh();
    let timer = 0;
    const timeout = new Promise<"timeout">((resolve) => { timer = window.setTimeout(() => resolve("timeout"), CONFIRM_TIMEOUT_S * 1000); });
    const result = await Promise.race([p, timeout]);
    window.clearTimeout(timer);
    // Fail-closed: no confirmation in time, or anything but a confirmed "on", stays off.
    setError(result === "timeout" ? `crew-service didn't confirm within ${CONFIRM_TIMEOUT_S} s, so it stays off.` : valueOf(result)?.status === "error" ? valueOf(result)?.error ?? "crew-service refused, so it stays off." : null);
    refresh();
  };
  const confirm = () => {
    const via = sheet?.via;
    setSheet(null);
    void run(via === "request" ? adapter.decide(crew, true) : adapter.turnOn(crew, duration));
  };

  const status = g?.status ?? "off";
  const on = status === "on" && !!g?.enforced?.length;
  const statement = !g ? "Read-only" : { off: "Read-only", error: "Read-only", pending: "Asking for files and terminal", applying: "Turning on files and terminal", on: "Files and terminal on" }[status];
  const detail = !g ? `${agents.join(" · ") || "No agents"} · ${lastRun}`
    : status === "pending" ? `${g.request?.role} asked at ${g.request?.requestedAt} · expires ${g.request?.expiresAt}`
      : status === "applying" ? "Approved by you · waiting for crew-service"
        : status === "on" ? `${g.expiresAt ? `Until ${g.expiresAt}` : "For this run"} · turned on by you at ${g.grantedAt}` : `${agents.join(" · ") || "No agents"} · ${lastRun}`;

  const sub = !g ? "Can't be turned on yet: crew-service doesn't report per-crew grants. This crew stays read-only."
    : { off: "Lets this crew read and write files in this run's folder and run terminal commands there. Off by default.", error: "Lets this crew read and write files in this run's folder and run terminal commands there. Off by default.", pending: "The crew asked for this. Nothing changes until you approve.", applying: "Approved by you. Waiting for crew-service to confirm it's enforcing the new allowlist.", on: `On for this crew only, inside ${g.scopePath}.` }[status];
  const footer = !g ? <>While off, this crew is read-only: it can only use <Code>kb_query</Code> and <Code>memory_query</Code>. Claude&apos;s crew backend enforces this, so the app can&apos;t override it.</>
    : status === "pending" ? `Approve or deny it in Crew approvals above. If you do nothing, the request expires in ${REQUEST_EXPIRY_MIN} minutes and the switch stays off.`
      : status === "applying" ? `The switch shows On only after the backend reports the allowlist it's enforcing. If it doesn't confirm in ${CONFIRM_TIMEOUT_S} s, it stays off and shows the error.`
        : status === "on" ? `Turns itself off ${g.expiresAt ? `at ${g.expiresAt}` : "when this run ends"}. Turning it off stops any terminal command that's still running.`
          : <>While off, this crew is read-only: it can only use <Code>kb_query</Code> and <Code>memory_query</Code>. Claude&apos;s crew backend enforces this, so the app can&apos;t override it.</>;
  const accessory = !g ? <DataTag kind="wire" />
    : status === "pending" ? <StatusText kind="wait">Waiting for you</StatusText>
      : status === "applying" ? <span role="status"><StatusText kind="run">Applying…</StatusText></span>
        : <Toggle checked={on} aria-label={`Files and terminal for ${crew}`} onCheckedChange={(next) => { if (next) { setDuration(DEFAULT_DURATION); setSheet({ via: "switch" }); } else void run(adapter.turnOff(crew)); }} />;

  return (
    <>
      <nav className="pk-crumb" aria-label="Breadcrumb"><Button className="cc-back" variant="plain" size="small" icon={ChevronLeft} onClick={onBack}>Crews</Button><span aria-hidden="true">/</span><span>{crew}</span></nav>
      <PageHero title="Crew" statement={`${crew} · ${statement}`} detail={detail} />
      {g?.status === "pending" && g.request && <RequestCard crew={crew} g={g} onDeny={() => { void run(adapter.decide(crew, false)); }} onReview={() => { setDuration(g.request!.duration); setSheet({ via: "request" }); }} />}
      {on && g && (
        <div className="pk-ok crew-tools__banner" role="status"><strong>On</strong><span>Files and terminal inside <Code>{g.scopePath}</Code> · {g.expiresAt ? `until ${g.expiresAt}` : "this run"}</span>
          <span className="crew-tools__banner-act"><Button size="small" onClick={() => { void run(adapter.turnOff(crew)); }}>Turn off now</Button></span></div>
      )}
      {error && <p className="pk-warn crew-tools__error" role="alert">{error}</p>}
      <div className="pk-g2">
        <div className="pk-stack">
          <Card title="Permissions" accessory={<span className="pk-acc">{wiredTag(grant)}</span>}>
            <div className="ui-inset-group crew-tools__group">
              <h3 className="ui-inset-group__title">Tools</h3>
              <ul className="ui-inset-group__list">
                <li className="ui-list-row"><div className="ui-list-row__main"><span className="ui-list-row__text"><span className="ui-list-row__title">Files and terminal</span><span className="ui-list-row__subtitle">{sub}</span></span></div><span className="ui-list-row__accessory">{accessory}</span></li>
              </ul>
              <p className="ui-inset-group__footer">{footer}</p>
            </div>
          </Card>
          <Card title="What this crew can use" accessory={on ? <span className="pk-acc"><StatusText kind="ok">Enforced by crew-service</StatusText></span> : undefined}>
            <Capabilities on={on} scope={g?.scopePath ?? "this run's folder"} />
          </Card>
        </div>
        <div className="pk-stack">
          <Card title="Agents">
            {agents.length === 0 ? <p className="pk-note">Agents appear after the crew&apos;s first run.</p> : agents.map((a) => (
              <div className="pk-row" key={a}><div className="pk-row__main"><span>{a}</span><small>{on ? "files and terminal (this crew)" : "read-only until the switch is on"}</small></div></div>
            ))}
          </Card>
          <Card title="Approver"><p className="pk-note">Only you can turn this on or approve a crew&apos;s request. Other agents and crews can ask, never grant.</p></Card>
        </div>
      </div>
      <Audit rows={adapter.audit(crew)} />
      <Dialog open={sheet !== null} title={`Give ${crew} files and terminal?`} confirmLabel={DURATION_ACTION[duration]} destructive className="crew-tools__sheet"
        onConfirm={confirm} onCancel={() => setSheet(null)}>
        <div className="crew-tools__sheet-body">
          <p>The crew&apos;s executor will be able to read and write files and run terminal commands without asking, inside the folder below.</p>
          <div className="pk-warn"><strong>⚠ This lets AI run commands on your Mac.</strong>
            <ul><li>It can change or break files in the folder. A git checkpoint is taken before each write.</li><li>Commands run as you, with a 60 s timeout. Secrets in output are redacted.</li><li>Still never allowed: outbound sends, spending, <Code>git push</Code>, sudo. Deleting anything still asks you each time.</li></ul></div>
          <div className="pk-field"><span>Folder · this run&apos;s folder</span><span className="pk-input">{g?.scopePath ?? "this run's folder"}</span></div>
          <div className="pk-field"><span id="crew-tools-duration">For how long</span>
            <SegmentedControl label="For how long" size="small" value={duration} onChange={setDuration} options={(["run", "1h", "24h"] as const).map((d) => ({ value: d, label: DURATION_LABEL[d] }))} /></div>
          <p className="pk-note">Logged as &ldquo;Taisun turned on files and terminal&rdquo; with the time, folder and duration. Claude&apos;s crew backend enforces it; the switch shows On only once the backend confirms.</p>
        </div>
      </Dialog>
    </>
  );
}
