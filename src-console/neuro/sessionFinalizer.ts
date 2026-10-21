// Keep successful stop steps and the exact save payload across retry attempts.
export class SessionFinalizer<T> {
  private streamStopped = false;
  private recordingStopped = false;
  private payload: { value: T } | null = null;
  private saved = false;
  private running: Promise<void> | null = null;

  run(steps: {
    stopStream: () => Promise<unknown>;
    stopRecording: () => Promise<unknown>;
    snapshot: () => T;
    save: (payload: T) => Promise<unknown>;
  }): Promise<void> {
    if (this.running) return this.running;
    if (this.saved) return Promise.resolve();
    this.running = (async () => {
      if (!this.streamStopped) {
        await steps.stopStream();
        this.streamStopped = true;
      }
      if (!this.recordingStopped) {
        await steps.stopRecording();
        this.recordingStopped = true;
      }
      this.payload ??= { value: steps.snapshot() };
      await steps.save(this.payload.value);
      this.saved = true;
    })().finally(() => { this.running = null; });
    return this.running;
  }
}
