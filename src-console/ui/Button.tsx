import type { LucideIcon } from "lucide-react";
import type { ComponentPropsWithRef, MouseEvent, ReactNode } from "react";
import { cx } from "./shared";
import "./Button.css";

export type ButtonVariant = "primary" | "secondary" | "plain" | "destructive";
export type ButtonSize = "regular" | "small";

type ButtonBase = Omit<ComponentPropsWithRef<"button">, "children"> & {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Shows a spinner, sets aria-busy and ignores clicks. The button keeps its width. */
  loading?: boolean;
};

type TextButtonProps = ButtonBase & { icon?: LucideIcon; iconOnly?: false; children: ReactNode };
/** Icon-only buttons have no visible text, so an aria-label is required. */
type IconOnlyButtonProps = ButtonBase & { icon: LucideIcon; iconOnly: true; "aria-label": string; children?: never };

export type ButtonProps = TextButtonProps | IconOnlyButtonProps;

export function Button({ variant = "secondary", size = "regular", icon: Icon, iconOnly = false, loading = false, className, children, type = "button", onClick, ...rest }: ButtonProps) {
  const handleClick = loading ? (event: MouseEvent<HTMLButtonElement>) => event.preventDefault() : onClick;
  return (
    <button
      {...rest}
      type={type}
      className={cx("ui-button", className)}
      data-variant={variant}
      data-size={size}
      data-icon-only={iconOnly || undefined}
      data-loading={loading || undefined}
      aria-busy={loading || undefined}
      aria-disabled={rest.disabled || rest["aria-disabled"] || undefined}
      onClick={handleClick}
    >
      <span className="ui-button__content">
        {Icon && <Icon className="ui-button__icon" aria-hidden="true" />}
        {!iconOnly && <span className="ui-button__label">{children}</span>}
      </span>
      {loading && <span className="ui-button__spinner" aria-hidden="true" />}
    </button>
  );
}

type LegacyVariant = "danger" | "quiet";
const LEGACY_VARIANTS: Record<LegacyVariant, ButtonVariant> = { danger: "destructive", quiet: "plain" };

export type ActionButtonProps = Omit<TextButtonProps, "variant"> & { variant?: ButtonVariant | LegacyVariant };

/** Migration alias for design/ProductUI's ActionButton: accepts its old "danger" / "quiet" variants. */
export function ActionButton({ variant = "secondary", ...props }: ActionButtonProps) {
  const mapped = variant === "danger" || variant === "quiet" ? LEGACY_VARIANTS[variant] : variant;
  return <Button {...props} variant={mapped} />;
}
