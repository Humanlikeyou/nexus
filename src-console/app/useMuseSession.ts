import { useCallback, useEffect, useRef, useState } from "react";
import { connectMuse, disconnectMuse, saveMuseSession, scanMuse, startEegRecording, startEegStream, stopEegRecording, stopEegStream } from "../bridge";
import { readableError } from "../lib/format";
import { RecordingSummarizer } from "../neuro/recordingSummary";
import { SessionFinalizer } from "../neuro/sessionFinalizer";
import { museTelemetry } from "../neuro/telemetryStore";
import { useTraining } from "../neuro/useTraining";
import type { MuseOperation } from "../screens/muse";
import type { MuseDevice, RecordedSession, SystemSnapshot } from "../types";

/**
 * Muse headset + recording session state and actions (scan, connect, start,
 * stop/save, disconnect). A plain hook, not a context: live telemetry stays
 * local to the component that calls it instead of fanning out app-wide.
 */
export function useMuseSession(data: SystemSnapshot, refresh: () => Promise<void>, setNotice: (notice: string | null) => void) {
  const [museDevices, setMuseDevices] = useState<MuseDevice[]>([]);
  const [museOperation, setMuseOperation] = useState<MuseOperation>(null);
  const [museError, setMuseError] = useState<string | null>(null);
  const [sessionStartedAt, setSessionStartedAt] = useState<number | null>(null);
  const [recordingFile, setRecordingFile] = useState<string | null>(null);
  const [sessionEndedAt, setSessionEndedAt] = useState<number | null>(null);
  const sessionFinalizer = useRef<SessionFinalizer<{ session: RecordedSession & { file_path: string }; data: Record<string, unknown> }> | null>(null);
  const sessionOperation = useRef(false);
  const sessionStopTime = useRef<number | null>(null);
  // Plain-recording summary (no training protocol): sampled from live telemetry while recording.
  const recordingSummarizer = useRef<RecordingSummarizer | null>(null);
  // Live telemetry lives in an external store; this hook never re-renders on packets.
  useEffect(() => museTelemetry.start(), []);
  const resetTelemetry = museTelemetry.reset;
  const readCounters = museTelemetry.readCounters;
  const training = useTraining(museTelemetry.getSnapshot, sessionStartedAt !== null && sessionEndedAt === null && data.muse.streaming);
  // Stable callbacks from useTraining, pulled out so the hooks below can list them as deps.
  const { summary: summarizeTraining, begin: beginTraining, finish: finishTraining } = training;
  const recording = sessionStartedAt !== null && sessionEndedAt === null;
  useEffect(() => {
    if (!recording) return;
    const timer = window.setInterval(() => recordingSummarizer.current?.tick(museTelemetry.getSnapshot(), Date.now()), 500);
    return () => window.clearInterval(timer);
  }, [recording]);

  const scanForMuse = useCallback(async () => {
    setMuseOperation("scanning");
    setMuseError(null);
    try {
      const found = await scanMuse();
      setMuseDevices(found);
      setNotice(found.length ? `${found.length} Muse headset${found.length === 1 ? "" : "s"} found` : "No Muse headsets found");
    } catch (error) {
      setMuseError(readableError(error));
    } finally {
      setMuseOperation(null);
      window.setTimeout(() => setNotice(null), 3000);
      await refresh();
    }
  }, [refresh, setNotice]);

  const connectToMuse = useCallback(async (device: MuseDevice) => {
    if (sessionStartedAt !== null || sessionOperation.current) {
      setMuseError("Stop and save the current session in Live before connecting another headset.");
      return;
    }
    setMuseOperation("connecting");
    setMuseError(null);
    try {
      await connectMuse(device.address);
      setMuseDevices([]);
      setNotice(`${device.name} connected`);
      await refresh();
    } catch (error) {
      setMuseError(readableError(error));
    } finally {
      setMuseOperation(null);
      window.setTimeout(() => setNotice(null), 3000);
    }
  }, [refresh, sessionStartedAt, setNotice]);

  const persistMuseSession = useCallback(async () => {
    if (sessionStartedAt === null || !recordingFile) {
      await stopEegStream();
      await stopEegRecording();
      return false;
    }
    const finalizer = sessionFinalizer.current ??= new SessionFinalizer();
    await finalizer.run({
      stopStream: async () => {
        await stopEegStream();
        sessionStopTime.current = Date.now();
        setSessionEndedAt(sessionStopTime.current);
      },
      stopRecording: stopEegRecording,
      snapshot: () => {
        const endedAt = sessionStopTime.current ?? Date.now();
        const counters = readCounters();
        const trainingSummary = summarizeTraining();
        const plain = recordingSummarizer.current?.summary() ?? null;
        const pick = <K extends "avg_focus" | "avg_relaxation" | "calm_pct">(key: K) => trainingSummary?.[key] ?? plain?.[key] ?? null;
        const session: RecordedSession & { file_path: string } = {
          id: typeof crypto?.randomUUID === "function" ? crypto.randomUUID() : `muse-${sessionStartedAt}`,
          device_type: "muse",
          start: new Date(sessionStartedAt).toISOString(),
          end: new Date(endedAt).toISOString(),
          duration_secs: Math.max(1, Math.round((endedAt - sessionStartedAt) / 1000)),
          data_points: counters.sampleCount,
          file_path: recordingFile,
          summary: { avg_focus: pick("avg_focus"), avg_relaxation: pick("avg_relaxation"), avg_hr: null, max_hr: null, avg_hrv: null, calm_pct: pick("calm_pct"), time_in_target_pct: trainingSummary?.time_in_target_pct ?? null },
        };
        finishTraining();
        return { session, data: {
          source: "live_muse",
          replay_file: recordingFile,
          packets: counters.packetCount,
          channel_samples: counters.channelSamples,
          neurofeedback: trainingSummary,
          recording_summary: trainingSummary ? null : plain,
        } };
      },
      save: (payload) => saveMuseSession(payload.session, payload.data),
    });
    return true;
  }, [recordingFile, sessionStartedAt, readCounters, summarizeTraining, finishTraining]);

  const startMuseSession = useCallback(async () => {
    if (sessionOperation.current || sessionStartedAt !== null || sessionFinalizer.current) return;
    sessionOperation.current = true;
    setMuseOperation("starting");
    setMuseError(null);
    resetTelemetry();
    let file: string | null = null;
    try {
      file = await startEegRecording();
      await startEegStream();
      const startedAt = Date.now();
      setRecordingFile(file);
      sessionStopTime.current = null;
      recordingSummarizer.current = new RecordingSummarizer();
      setSessionStartedAt(startedAt);
      beginTraining();
      setNotice("Muse session started");
      await refresh();
    } catch (error) {
      if (file) await stopEegRecording().catch(() => undefined);
      setMuseError(readableError(error));
    } finally {
      sessionOperation.current = false;
      setMuseOperation(null);
      window.setTimeout(() => setNotice(null), 3000);
    }
  }, [refresh, resetTelemetry, beginTraining, sessionStartedAt, setNotice]);

  const stopMuseSession = useCallback(async () => {
    if (sessionOperation.current) return;
    sessionOperation.current = true;
    setMuseOperation("stopping");
    setMuseError(null);
    try {
      const saved = await persistMuseSession();
      finishTraining();
      setSessionStartedAt(null);
      setRecordingFile(null);
      setSessionEndedAt(null);
      sessionFinalizer.current = null;
      setNotice(saved ? "Muse session saved" : "Muse stream stopped");
    } catch (error) {
      setMuseError(`Session could not be finalized. Keep Amara OS open and retry. ${readableError(error)}`);
    } finally {
      sessionOperation.current = false;
      setMuseOperation(null);
      await refresh();
      window.setTimeout(() => setNotice(null), 3000);
    }
  }, [persistMuseSession, refresh, finishTraining, setNotice]);

  const disconnectFromMuse = useCallback(async () => {
    if (sessionOperation.current) return;
    sessionOperation.current = true;
    setMuseOperation("disconnecting");
    setMuseError(null);
    try {
      if (data.muse.streaming || sessionStartedAt !== null) {
        await persistMuseSession();
      }
      await disconnectMuse();
      finishTraining();
      setSessionStartedAt(null);
      setRecordingFile(null);
      setSessionEndedAt(null);
      sessionFinalizer.current = null;
      resetTelemetry();
      setNotice("Muse disconnected");
    } catch (error) {
      setMuseError(readableError(error));
    } finally {
      sessionOperation.current = false;
      setMuseOperation(null);
      await refresh();
      window.setTimeout(() => setNotice(null), 3000);
    }
  }, [data.muse.streaming, sessionStartedAt, persistMuseSession, refresh, resetTelemetry, finishTraining, setNotice]);

  return { museDevices, museOperation, museError, sessionStartedAt, sessionEndedAt, training, scanForMuse, connectToMuse, disconnectFromMuse, startMuseSession, stopMuseSession };
}
