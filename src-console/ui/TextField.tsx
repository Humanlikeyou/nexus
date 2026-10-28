import type { LucideIcon } from "lucide-react";
import { useId, type ComponentPropsWithRef, type ReactNode } from "react";
import { cx, joinIds } from "./shared";
import "./TextField.css";

export type TextFieldProps = Omit<ComponentPropsWithRef<"input">, "children"> & {
  label: string;
  /** Keeps the label for assistive tech but hides it visually. */
  hideLabel?: boolean;
  description?: ReactNode;
  /** Error message. When set, the input is aria-invalid and described by it. */
  error?: ReactNode;
  leadingIcon?: LucideIcon;
};

export function TextField({ label, hideLabel = false, description, error, leadingIcon: Icon, className, id, type = "text", "aria-describedby": describedBy, ...rest }: TextFieldProps) {
  const autoId = useId();
  const inputId = id ?? `${autoId}-input`;
  const descriptionId = description ? `${autoId}-description` : undefined;
  const errorId = error ? `${autoId}-error` : undefined;
  return (
    <div className={cx("ui-text-field", className)} data-invalid={error ? true : undefined} data-has-icon={Icon ? true : undefined}>
      <label className={cx("ui-text-field__label", hideLabel && "ui-visually-hidden")} htmlFor={inputId}>{label}</label>
      <div className="ui-text-field__control">
        {Icon && <Icon className="ui-text-field__icon" aria-hidden="true" />}
        <input
          {...rest}
          id={inputId}
          type={type}
          className="ui-text-field__input"
          aria-invalid={error ? true : undefined}
          aria-describedby={joinIds(describedBy, descriptionId, errorId)}
        />
      </div>
      {description && <p id={descriptionId} className="ui-text-field__description">{description}</p>}
      {error && <p id={errorId} className="ui-text-field__error">{error}</p>}
    </div>
  );
}
