import type { SystemSnapshot } from "../types";
import { Badge, Card } from "../ui";

/** Chat & Agents (route "chat"). Placeholder until the three-pane screen lands. */
export function ChatField({ data }: { data: SystemSnapshot }) {
  const online = data.hermes.status === "online";
  return (
    <div className="chat-screen chat-screen--placeholder">
      <h1>Chat &amp; Agents</h1>
      <Card title="Terrance" accessory={<Badge tone={online ? "success" : "warning"}>{online ? "Online" : "Offline"}</Badge>}>
        <p>Conversations with Terrance and your agents will live here.</p>
      </Card>
    </div>
  );
}
