/** Settings section tabs (design-1111s; Taisun, 14:59 JST Nov 11), the same tab bar Business uses. */
export const SETTINGS_TABS = ["main", "services", "devices", "notifications", "backup", "voice", "memory"] as const;
/** Old anchors that now live inside a tab. */
const TAB_OF: Record<string, string> = { "home-assistant": "devices", about: "main", updates: "main" };
export function settingsTab(view = "main"): string {
  const v = TAB_OF[view] ?? view;
  return (SETTINGS_TABS as readonly string[]).includes(v) ? v : "main";
}
