import { useState } from "react";
import { Button, DataTag, InsetGroup, ListRow, PageHero, Toggle } from "../ui";
import { valueOf } from "../lib/sampleAdapters";
import { ALERT_KINDS, type AlertKind, type Outcome, type SecretName, type StandaloneAdapter } from "./adapters";
import { KeyState, SaCard, WireNote } from "./parts";
import { SecretSheet } from "./SecretSheet";

/** Plain-words result of Send Test (decision 0042): outbound off, declined, missing keys, or a Telegram error each say so. */
export function sendTestNote(r: Outcome): string {
  switch (r.state) {
    case "done": return "Test message sent. Check your Telegram.";
    case "off": return "Nothing was sent: outbound is disabled for this session. Turn it back on and try again.";
    case "declined": return "Cancelled. Nothing was sent.";
    case "busy": return "Another approval is still open. Finish it, then try again. Nothing was sent.";
    case "expired": return "The confirmation timed out. Nothing was sent; try again.";
    case "missing": return "Add your bot token and chat first. Nothing was sent.";
    case "telegram": return "Telegram didn't accept the message. Check your bot token and chat ID. Nothing was sent.";
    case "unwired": return "Test messages aren't connected in this view. Nothing was sent.";
    default: return "The test message didn't go through. Nothing was sent.";
  }
}

/**
 * Settings › Notifications (design-1111s s5a/s5b): Amara's health alerts relayed to Telegram.
 * Taisun (14:59 JST Nov 11): no quiet hours and no rate limit. Only the switch, the keys, the kinds and the preview.
 */
export function NotificationsView({ adapter }: { adapter: StandaloneAdapter }) {
  const [, bump] = useState(0);
  const [sheet, setSheet] = useState<SecretName | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [writing, setWriting] = useState(false);
  const rw = adapter.relay();
  const relay = valueOf(rw);
  const keys = valueOf(adapter.secrets());
  const token = keys?.find((k) => k.name === "telegram_bot_token")?.set ?? false;
  const chat = keys?.find((k) => k.name === "telegram_chat_id")?.set ?? false;
  const pw = adapter.relayPreview();
  const preview = valueOf(pw);
  const ready = token && chat;
  const on = !!relay?.enabled && ready;
  const set = async (p: Parameters<StandaloneAdapter["setRelay"]>[0]) => { const r = await adapter.setRelay(p); if (r.state === "unwired") setNote("Saving needs wiring."); bump((n) => n + 1); };
  const another = async () => { setWriting(true); const r = await adapter.writeAnotherPreview(); setWriting(false); setNote(r.state === "done" ? "Wrote a new preview. Nothing was sent." : r.state === "unwired" ? "Previews aren't connected in this view. Nothing changed." : r.message ?? "Couldn't write another one right now."); bump((n) => n + 1); };
  const savedNote = (name: SecretName) => { const other = name === "telegram_bot_token" ? chat : token; setNote(other ? "Saved to the Keychain. Both keys are set: turn the relay on, then Send Test." : "Saved to the Keychain. Add the other key to turn the relay on."); };
  const kind = (k: AlertKind) => relay?.kinds[k] ?? false;
  const notYet = (k: AlertKind) => !!relay?.notConnected?.includes(k);
  return (
    <div className="pk-host sa-page">
      <PageHero title="Notifications" statement={!relay ? "Telegram relay" : on ? "Telegram relay is on" : "Telegram relay is off"}
        detail={!relay ? <>Your health alerts in Amara&apos;s words, sent to your Telegram <DataTag kind="wire" /></> : on ? `Health alerts in Amara's words${relay.lastSent ? ` · last sent ${relay.lastSent}` : ""}` : ready ? "Turn it on to send health alerts to your Telegram" : "Add your bot token and chat to turn it on"} />
      <div className="pk-g2">
        <div className="pk-stack">
          <InsetGroup title="Health alerts to Telegram" footer={relay ? "Telegram is the only thing this switch sends off the Mac. Turning it off stops all sends at once. Tests never send while outbound is disabled." : undefined}>
            <ListRow title="Send health alerts to Telegram" subtitle={ready && relay && !relay.enabled ? "Ready · turn it on. Amara rewrites each alert in her own words from today's data, on this Mac." : "Amara rewrites each alert in her own words from today's data, on this Mac. Cloud AI stays off."}
              accessory={<Toggle aria-label="Telegram relay" checked={on} disabled={!relay || !ready} onCheckedChange={(v) => { void set({ enabled: v }); }} />} />
            <ListRow title="Bot token" subtitle="From @BotFather · kept in the Keychain"
              accessory={<span className="settings__row-actions">{keys ? <KeyState set={token} /> : <DataTag kind="wire" />}<Button size="small" variant={token ? "secondary" : "primary"} onClick={() => setSheet("telegram_bot_token")}>{token ? "Change…" : "Add…"}</Button></span>} />
            <ListRow title="Your chat" subtitle={chat && !on ? "Only this chat receives messages · Turn the relay on to send a test" : "Only this chat receives messages"}
              accessory={<span className="settings__row-actions">{keys ? <KeyState set={chat} /> : <DataTag kind="wire" />}{chat ? <Button size="small" variant="secondary" disabled={!on} onClick={() => { void (async () => { const r = await adapter.relaySendTest(); setNote(sendTestNote(r)); bump((n) => n + 1); })(); }}>Send Test…</Button> : <Button size="small" variant="secondary" onClick={() => setSheet("telegram_chat_id")}>Add…</Button>}</span>} />
          </InsetGroup>
          <SaCard title="What gets sent" w={rw}>
            {relay ? (
              <div className="pk-chips" role="group" aria-label="Alerts to send">
                {ALERT_KINDS.map((k) => <button key={k.id} type="button" className="pk-chip sa-chip" data-on={kind(k.id) || undefined} aria-pressed={kind(k.id)} disabled={notYet(k.id)} onClick={() => { void set({ kinds: { [k.id]: !kind(k.id) } as Record<AlertKind, boolean> }); }}>{k.label}{notYet(k.id) ? " (not connected yet)" : ""}</button>)}
              </div>
            ) : <WireNote>Needs wiring: pick sleep, recovery, effort, strap battery, sync problems, Lucid cues and illness signs once the relay is built (HANDOFF-1111s §5).</WireNote>}
            {relay && <p className="pk-note">Each alert goes out as it happens. Lucid cues are off to start.</p>}
          </SaCard>
          {note && <p className="pk-note" role="status">{note}</p>}
        </div>
        <div className="pk-stack">
          <SaCard title="Preview" w={pw} accessory={preview ? <button className="pk-link" type="button" disabled={writing} onClick={() => { void another(); }}>{writing ? "Writing…" : "Write another ›"}</button> : undefined}>
            {preview ? <>
              <p className="pk-note">{preview.from}</p>
              <div className="pk-tg"><div className="pk-tg__head"><span className="pk-tg__av" aria-hidden="true" /><b className="sa-label">Amara</b><span>bot · Telegram</span></div>
                <div className="pk-bubble">{preview.text}<time>{preview.at}</time></div></div>
              <p className="pk-note">Numbers come from your health data and are checked against it before sending. If a number can&apos;t be checked, Amara leaves it out.</p>
            </> : <WireNote>Needs wiring: Amara&apos;s light model writes each message from that day&apos;s data. Nothing is sent from here.</WireNote>}
          </SaCard>
        </div>
      </div>
      <SecretSheet open={sheet === "telegram_bot_token"} name="telegram_bot_token" title="Telegram bot token" intro="Paste the token @BotFather gave you. It goes straight into this Mac's Keychain." set={token} adapter={adapter}
        onClose={(saved, wired) => { setSheet(null); if (!wired) setNote("Saving to the Keychain needs wiring. Nothing was stored."); else if (saved) savedNote("telegram_bot_token"); bump((n) => n + 1); }} />
      <SecretSheet open={sheet === "telegram_chat_id"} name="telegram_chat_id" title="Telegram chat" intro="Paste your chat ID. Only this chat receives messages." set={chat} adapter={adapter}
        onClose={(saved, wired) => { setSheet(null); if (!wired) setNote("Saving to the Keychain needs wiring. Nothing was stored."); else if (saved) savedNote("telegram_chat_id"); bump((n) => n + 1); }} />
    </div>
  );
}
