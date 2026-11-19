/** "Open goal ›" from a take or card that has no router of its own: App listens and opens Life › Goals with that goal in focus. */
export const OPEN_GOAL_EVENT = "amara:open-goal";
export function openGoal(goalId: string) {
  try { window.dispatchEvent(new CustomEvent<{ goalId: string }>(OPEN_GOAL_EVENT, { detail: { goalId } })); } catch { /* no window in node tests */ }
}
