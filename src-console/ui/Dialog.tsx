import { useEffect, useId, useRef, type ReactNode } from "react";
import { Button } from "./Button";
import { cx } from "./shared";
import "./Dialog.css";

export type DialogProps = {
  open: boolean;
  title: string;
  /** Body copy; plain text or nodes. */
  children?: ReactNode;
  /** Omit (with onConfirm) for a content-only dialog such as the ⌘K palette: no action row. */
  confirmLabel?: string;
  cancelLabel?: string;
  /** Red confirm button for actions that delete or overwrite. */
  destructive?: boolean;
  /** Disables both buttons and shows a spinner on confirm while work runs. */
  busy?: boolean;
  onConfirm?: () => void;
  /** Cancel button, Esc and backdrop-less close all route here. */
  onCancel: () => void;
  className?: string;
  /** Keep the title for assistive tech only. */
  hideTitle?: boolean;
};

/** Thin confirm dialog over the native <dialog> (macOS 13 WebKit; no Popover API).
 *  Initial focus is on Cancel, Esc = Cancel, focus returns to the opener on close. */
export function Dialog({ open, title, children, confirmLabel, cancelLabel = "Cancel", destructive = false, busy = false, onConfirm, onCancel, className, hideTitle = false }: DialogProps) {
  const ref = useRef<HTMLDialogElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const opener = useRef<Element | null>(null);
  const pressed = useRef<Element | null>(null);
  const titleId = useId();
  const bodyId = useId();

  // WebKit does not focus a button on click, so activeElement cannot name a mouse opener.
  useEffect(() => {
    const note = (event: PointerEvent) => { pressed.current = event.target instanceof Element ? event.target.closest("button") : null; };
    document.addEventListener("pointerdown", note, true);
    return () => document.removeEventListener("pointerdown", note, true);
  }, []);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      const active = document.activeElement;
      opener.current = active && active !== document.body ? active : pressed.current;
      if (typeof dialog.showModal === "function") dialog.showModal();
      else dialog.setAttribute("open", "");
      cancelRef.current?.focus();
    } else if (!open && dialog.open) {
      if (typeof dialog.close === "function") dialog.close();
      else dialog.removeAttribute("open");
      if (opener.current instanceof HTMLElement) opener.current.focus();
    }
  }, [open]);

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    // Native Esc fires "cancel"; keep the dialog controlled by `open`.
    const onNativeCancel = (event: Event) => { event.preventDefault(); if (!busy) onCancel(); };
    dialog.addEventListener("cancel", onNativeCancel);
    return () => dialog.removeEventListener("cancel", onNativeCancel);
  }, [busy, onCancel]);

  return (
    <dialog ref={ref} className={cx("ui-dialog", className)} aria-labelledby={titleId} aria-describedby={children ? bodyId : undefined} data-destructive={destructive || undefined}>
      <h2 id={titleId} className={hideTitle ? "ui-visually-hidden" : "ui-dialog__title"}>{title}</h2>
      {children && <div id={bodyId} className="ui-dialog__body">{children}</div>}
      {confirmLabel && onConfirm && (
        <div className="ui-dialog__actions">
          <Button ref={cancelRef} data-initial-focus="" variant="secondary" onClick={onCancel} disabled={busy}>{cancelLabel}</Button>
          <Button variant={destructive ? "destructive" : "primary"} onClick={onConfirm} loading={busy}>{confirmLabel}</Button>
        </div>
      )}
    </dialog>
  );
}
