import type { FollowupItem, FollowupItemPatch } from "../types";
import { date } from "./format";

/** The fixed error kinds of commands/followups.rs, plus the two the UI adds. */
export type FollowupErrorKind = "invalid_input" | "not_found" | "malformed_file" | "unsafe_storage" | "storage_error" | "backend_missing" | "unknown";

const BACKEND_KINDS: FollowupErrorKind[] = ["invalid_input", "not_found", "malformed_file", "unsafe_storage", "storage_error"];

/** Plain-language copy. Rust's error text is never shown raw. */
export const FOLLOWUP_ERROR_COPY: Record<FollowupErrorKind, string> = {
  invalid_input: "That couldn't be saved. Check the text and the date, then try again.",
  not_found: "That's no longer on the list.",
  malformed_file: "Amara's list couldn't be read, so nothing was changed.",
  unsafe_storage: "Amara's list files have unexpected permissions, so nothing was changed.",
  storage_error: "Amara's list files couldn't be reached. Try again.",
  backend_missing: "Lists aren't in this version of Amara OS yet. Update the app, then come back here.",
  unknown: "Something went wrong. Try again.",
};

export function followupErrorKind(error: unknown): FollowupErrorKind {
  const raw = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  const objectKind = typeof error === "object" && error !== null && "kind" in error ? String((error as { kind: unknown }).kind) : "";
  const kind = objectKind || raw.split(":")[0].trim();
  const known = BACKEND_KINDS.find((candidate) => candidate === kind);
  if (known) return known;
  return /command .*not found|unknown command|not mocked/i.test(raw) ? "backend_missing" : "unknown";
}

const ISO_DAY = /^(\d{4})-(\d{2})-(\d{2})$/;

export const isIsoDay = (value: string | null | undefined) => typeof value === "string" && ISO_DAY.test(value);

/** Today on this Mac, in the form the lists use. */
export function todayIso(now = new Date()) {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/** "Oct 28" for a list date. Parsed as a local day so it can't slip a day. Amara's other text passes through. */
export function dayLabel(value: string) {
  const match = ISO_DAY.exec(value);
  if (!match) return value;
  return date(new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3])).getTime());
}

export type DueState = "none" | "overdue" | "today" | "later";

export function dueState(due: string | null, today: string): DueState {
  if (!isIsoDay(due)) return "none";
  if (due === today) return "today";
  return (due as string) < today ? "overdue" : "later";
}

export type ItemDraft = { text: string; due: string; next: string };

export const draftFrom = (item: FollowupItem): ItemDraft => ({ text: item.text, due: item.due ?? "", next: item.next ?? "" });

/** Only what the person changed, so a field Amara updated meanwhile isn't overwritten. */
export function itemPatch(base: FollowupItem, draft: ItemDraft): FollowupItemPatch {
  const start = draftFrom(base);
  const patch: FollowupItemPatch = {};
  const text = draft.text.trim();
  const next = draft.next.trim();
  if (text !== start.text) patch.text = text;
  if (draft.due !== start.due) patch.due = draft.due || null;
  if (next !== start.next) patch.next = next || null;
  return patch;
}

/** The backend strips hidden characters and sanitizes what it stores, so the saved text can differ from what was sent. */
export function changedBySave(sent: { text?: string; next?: string | null }, saved: FollowupItem) {
  if (sent.text !== undefined && sent.text !== saved.text) return true;
  return sent.next !== undefined && (sent.next || null) !== saved.next;
}

export const CLEANED_NOTE = "Saved. Some of the text was cleaned up for safety, so it may look different from what you typed.";

const chars = (value: string) => [...value].length;
export const MAX_TEXT_CHARS = 500;
export const MIN_TOPIC_CHARS = 3;
export const MAX_TOPIC_CHARS = 200;

export function textError(text: string) {
  if (!text.trim()) return "Add some text first.";
  return chars(text.trim()) > MAX_TEXT_CHARS ? `Keep this to ${MAX_TEXT_CHARS} characters or fewer.` : undefined;
}

export const nextError = (next: string) => (chars(next.trim()) > MAX_TEXT_CHARS ? `Keep this to ${MAX_TEXT_CHARS} characters or fewer.` : undefined);

export function topicError(topic: string) {
  const length = chars(topic.trim());
  if (length < MIN_TOPIC_CHARS) return `Use at least ${MIN_TOPIC_CHARS} characters so Amara doesn't mute too much.`;
  return length > MAX_TOPIC_CHARS ? `Keep this to ${MAX_TOPIC_CHARS} characters or fewer.` : undefined;
}

export const INCOMPLETE_DATE = "Finish the date, or clear it.";
