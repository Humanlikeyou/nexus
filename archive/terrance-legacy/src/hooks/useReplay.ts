import { useState, useCallback, useEffect } from "react";
import {
  startEegRecording,
  stopEegRecording,
  listReplays,
  startReplayFile,
  stopReplay as apiStopReplay,
  startDemoEeg,
  stopDemoEeg,
} from "@/lib/api";
import type { ReplayMode, ReplayFileInfo } from "@/lib/types";

export function useReplay() {
  const [mode, setMode] = useState<ReplayMode>("off");
  const [replayFiles, setReplayFiles] = useState<ReplayFileInfo[]>([]);
  const [speed, setSpeed] = useState(1);
  const [activeFilename, setActiveFilename] = useState<string | null>(null);
  const [activeDemoMode, setActiveDemoMode] = useState<string | null>(null);
  const [recordingStartedAt, setRecordingStartedAt] = useState<number | null>(null);

  const loadReplays = useCallback(async () => {
    try {
      const files = await listReplays();
      setReplayFiles(files);
    } catch (e) {
      console.error("[useReplay] loadReplays error:", e);
    }
  }, []);

  // Load replay files on mount
  useEffect(() => {
    loadReplays();
  }, [loadReplays]);

  const startRecording = useCallback(async () => {
    try {
      const filename = await startEegRecording();
      console.log("[useReplay] recording started:", filename);
      setMode("recording");
      setRecordingStartedAt(Date.now());
      return filename;
    } catch (e) {
      console.error("[useReplay] startRecording error:", e);
      throw e;
    }
  }, []);

  const stopRecording = useCallback(async () => {
    try {
      await stopEegRecording();
      console.log("[useReplay] recording stopped");
      setMode("off");
      setRecordingStartedAt(null);
      // Refresh file list
      await loadReplays();
    } catch (e) {
      console.error("[useReplay] stopRecording error:", e);
      throw e;
    }
  }, [loadReplays]);

  const startReplay = useCallback(
    async (filename: string, replaySpeed?: number) => {
      const s = replaySpeed ?? speed;
      try {
        await startReplayFile(filename, s);
        console.log("[useReplay] replay started:", filename, "speed:", s);
        setMode("replaying");
        setActiveFilename(filename);
      } catch (e) {
        console.error("[useReplay] startReplay error:", e);
        throw e;
      }
    },
    [speed],
  );

  const stopReplay = useCallback(async () => {
    try {
      await apiStopReplay();
      console.log("[useReplay] replay stopped");
      setMode("off");
      setActiveFilename(null);
    } catch (e) {
      console.error("[useReplay] stopReplay error:", e);
    }
  }, []);

  const startDemo = useCallback(
    async (demoMode: string, demoSpeed?: number) => {
      const s = demoSpeed ?? speed;
      try {
        await startDemoEeg(demoMode, s);
        console.log("[useReplay] demo started:", demoMode, "speed:", s);
        setMode("demo");
        setActiveDemoMode(demoMode);
      } catch (e) {
        console.error("[useReplay] startDemo error:", e);
        throw e;
      }
    },
    [speed],
  );

  const stopDemo = useCallback(async () => {
    try {
      await stopDemoEeg();
      console.log("[useReplay] demo stopped");
      setMode("off");
      setActiveDemoMode(null);
    } catch (e) {
      console.error("[useReplay] stopDemo error:", e);
    }
  }, []);

  return {
    mode,
    replayFiles,
    speed,
    setSpeed,
    activeFilename,
    activeDemoMode,
    recordingStartedAt,
    startRecording,
    stopRecording,
    startReplay,
    stopReplay,
    startDemo,
    stopDemo,
    loadReplays,
  };
}
