import { useCallback, useEffect, useRef, useState } from "react";
import { PROTOCOLS } from "../../src/lib/neurofeedback/protocols";
import { INACTIVE_TRAINING, TrainingSession } from "./training";
import type { MuseTelemetry } from "./telemetryStore";

/** `readTelemetry` is read on each 500 ms tick (no React subscription to live data). */
export function useTraining(readTelemetry: () => MuseTelemetry, active: boolean) {
  const [protocolId, setProtocolId] = useState("off");
  const [state, setState] = useState(INACTIVE_TRAINING);
  const session = useRef<TrainingSession | null>(null);
  const live = useRef({ readTelemetry, active });
  useEffect(() => { live.current = { readTelemetry, active }; }, [readTelemetry, active]);

  const begin = useCallback(() => {
    const protocol = PROTOCOLS.find((item) => item.id === protocolId);
    session.current = protocol ? new TrainingSession(protocol) : null;
    setState(session.current?.state ?? INACTIVE_TRAINING);
  }, [protocolId]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      if (live.current.active && session.current) {
        setState({ ...session.current.tick(live.current.readTelemetry(), Date.now()) });
      }
    }, 500);
    return () => window.clearInterval(timer);
  }, []);

  const summary = useCallback(() => session.current?.summary() ?? null, []);
  const finish = useCallback(() => { session.current = null; setState(INACTIVE_TRAINING); }, []);
  return { protocolId, setProtocolId, state, begin, summary, finish, protocols: PROTOCOLS };
}
