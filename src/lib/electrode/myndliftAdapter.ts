import type { ElectrodeDeviceInfo, DeviceTelemetry } from "@/lib/types";

/**
 * Adapter interface for electrode devices (Myndlift and future devices).
 *
 * Modeled after patterns found in:
 * - MuseLSL2 (Python/bleak): scan → connect → subscribe → start('d') → callbacks
 * - OpenWhoop (Rust/btleplug): scan → connect → subscribe → read notifications
 *
 * The interface mirrors the BLE lifecycle: discover → connect → stream → disconnect.
 * Data flows via callbacks registered before streaming starts.
 */
export interface ElectrodeAdapter {
  readonly name: string;

  /** Scan for nearby devices (filters by adapter-specific name pattern) */
  scan(timeoutMs?: number): Promise<ElectrodeDeviceInfo[]>;

  /** Connect to a specific device by ID/address */
  connect(deviceId: string): Promise<boolean>;

  /** Disconnect from current device */
  disconnect(): Promise<void>;

  /**
   * Start streaming EEG data.
   * Data arrives via the onData callback registered with setDataCallback.
   * Mirrors Muse protocol: subscribe to BLE characteristics, then send 'd' command.
   */
  startStream(): Promise<void>;

  /** Stop streaming (Muse sends 'h' command) */
  stopStream(): Promise<void>;

  /**
   * Request device telemetry (battery, firmware, serial, channel info).
   * Mirrors Muse 's' (status) and 'v1' (version) control commands.
   */
  getTelemetry(): Promise<DeviceTelemetry | null>;

  /** Get current connection status */
  isConnected(): boolean;

  /** Get current streaming status */
  isStreaming(): boolean;

  /**
   * Register a callback for incoming EEG data.
   * Signature matches Muse pattern: channel name + array of samples.
   */
  setDataCallback(cb: ((channel: string, samples: number[]) => void) | null): void;
}

/**
 * Muse device constants — derived from MuseLSL2 source.
 *
 * Muse 2 / Muse S EEG:
 *   5 channels (TP9, AF7, AF8, TP10, Right AUX)
 *   256 Hz sample rate
 *   12-bit resolution (0.488 µV/LSB, 2 mVpp range)
 *   12 samples per BLE packet per channel
 *
 * Additional sensors (not yet used):
 *   PPG: 3 channels (LUX, IR, RED) at 64 Hz
 *   ACC: 3 axes at 52 Hz
 *   GYRO: 3 axes at 52 Hz
 *   Telemetry: battery %, temperature, ADC voltage
 */
export const MUSE_CHANNELS = ["TP9", "AF7", "AF8", "TP10", "AUX"] as const;
export const MUSE_SAMPLE_RATE = 256;
export const MUSE_RESOLUTION_BITS = 12;
export const MUSE_UV_PER_LSB = 0.48828125; // 2000µV / 4096

/**
 * Stub adapter — all operations are no-ops that log to console.
 * Replace with real Myndlift BLE implementation when SDK is integrated.
 *
 * The stub follows the same lifecycle as the real adapter would:
 * scan() → connect() → setDataCallback() → startStream() → stopStream() → disconnect()
 */
export class StubMyndliftAdapter implements ElectrodeAdapter {
  readonly name = "myndlift-stub";
  private _connected = false;
  private _streaming = false;
  private _dataCallback: ((channel: string, samples: number[]) => void) | null = null;

  async scan(timeoutMs = 10_000): Promise<ElectrodeDeviceInfo[]> {
    console.log(`[myndlift-stub] scan(${timeoutMs}ms) — returning empty device list`);
    return [];
  }

  async connect(deviceId: string): Promise<boolean> {
    console.log(`[myndlift-stub] connect(${deviceId}) — stub, returning false`);
    return false;
  }

  async disconnect(): Promise<void> {
    console.log("[myndlift-stub] disconnect()");
    this._connected = false;
    this._streaming = false;
    this._dataCallback = null;
  }

  async startStream(): Promise<void> {
    console.log("[myndlift-stub] startStream() — stub, no-op");
  }

  async stopStream(): Promise<void> {
    console.log("[myndlift-stub] stopStream() — stub, no-op");
    this._streaming = false;
  }

  async getTelemetry(): Promise<DeviceTelemetry | null> {
    console.log("[myndlift-stub] getTelemetry() — returning null");
    return null;
  }

  isConnected(): boolean {
    return this._connected;
  }

  isStreaming(): boolean {
    return this._streaming;
  }

  setDataCallback(cb: ((channel: string, samples: number[]) => void) | null): void {
    this._dataCallback = cb;
  }
}
