/**
 * Quiet placeholder while a lazy screen chunk loads. Invisible for the first
 * ~150 ms (CSS animation-delay), so fast loads never flash anything.
 */
export function ScreenSkeleton() {
  return (
    <div className="screen-skeleton" role="status" aria-label="Loading">
      <span className="screen-skeleton__title" />
      <span className="screen-skeleton__line" />
      <span className="screen-skeleton__line screen-skeleton__line--short" />
      <span className="screen-skeleton__block" />
    </div>
  );
}
