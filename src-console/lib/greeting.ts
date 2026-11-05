/** Home's welcome. It changes with the time of day and the date, never between renders. */
const LINES = {
  morning: ["Good morning, Taisun", "Morning, Taisun", "A fresh start, Taisun"],
  afternoon: ["Good afternoon, Taisun", "Afternoon, Taisun", "Welcome back, Taisun"],
  evening: ["Good evening, Taisun", "Evening, Taisun", "Welcome back, Taisun"],
  night: ["Good evening, Taisun", "Still up, Taisun?", "A quiet hour, Taisun"],
} as const;

export function dayPart(hour: number): keyof typeof LINES {
  if (hour >= 5 && hour < 12) return "morning";
  if (hour >= 12 && hour < 17) return "afternoon";
  if (hour >= 17 && hour < 22) return "evening";
  return "night";
}

/** The classic line most days; a variation now and then, the same one all morning. */
export function localGreeting(now: Date): string {
  const part = dayPart(now.getHours());
  const day = Math.floor(Date.UTC(now.getFullYear(), now.getMonth(), now.getDate()) / 86_400_000);
  const lines = LINES[part];
  const turn = day % 5;
  return lines[turn < lines.length - 1 ? turn : 0];
}
