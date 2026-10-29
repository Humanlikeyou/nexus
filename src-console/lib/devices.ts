/**
 * Placeholder device id the backend reports before a real NOOP band is known. Internal only:
 * it must never be shown (Decision 10). Single source for bridge, Fitness and Devices.
 */
export const EMPTY_DEVICE_ID = "my-whoop";

/** The NOOP band's display title: its real id, or "NOOP band" for none or the placeholder. */
export function deviceTitle(deviceId: string | null | undefined): string {
  return deviceId && deviceId !== EMPTY_DEVICE_ID ? deviceId : "NOOP band";
}
