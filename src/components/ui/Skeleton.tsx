import { cn } from "@/lib/utils";

interface SkeletonProps {
  variant?: "text" | "circle" | "rect" | "chart";
  width?: string | number;
  height?: string | number;
  className?: string;
}

export function Skeleton({
  variant = "text",
  width,
  height,
  className,
}: SkeletonProps) {
  const styles: Record<string, string> = {
    text: "h-3 w-full rounded",
    circle: "w-8 h-8 rounded-full",
    rect: "w-full h-20 rounded-lg",
    chart: "w-full h-[180px] rounded-lg",
  };

  return (
    <div
      className={cn(
        "animate-pulse bg-muted/20",
        styles[variant],
        className,
      )}
      style={{
        width: width ?? undefined,
        height: height ?? undefined,
      }}
    />
  );
}

export function SkeletonGroup({ lines = 3 }: { lines?: number }) {
  return (
    <div className="space-y-2">
      {Array.from({ length: lines }).map((_, i) => (
        <Skeleton
          key={i}
          variant="text"
          width={i === lines - 1 ? "60%" : undefined}
        />
      ))}
    </div>
  );
}
