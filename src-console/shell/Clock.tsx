import { useNow } from "./useNow";

/** Toolbar date + time. Owns the 1 s tick so the app root never re-renders for it. */
export function Clock() {
  const now = useNow();
  return <>
    <span>{now.toLocaleDateString([], { month: "short", day: "numeric" })}</span>
    <strong>{now.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })}</strong>
  </>;
}
