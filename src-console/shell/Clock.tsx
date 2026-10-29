import { date, time } from "../lib/format";
import { useNow } from "./useNow";

/** Toolbar date + time. Owns the 1 s tick so the app root never re-renders for it. */
export function Clock() {
  const now = useNow();
  return <>
    <span>{date(now)}</span>
    <strong>{time(now)}</strong>
  </>;
}
