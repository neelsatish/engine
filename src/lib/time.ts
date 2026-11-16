// Local times in Engine's format ("YYYY-MM-DDTHH:MM"), shared by the Schedule and the Briefing.

export type Block = { id: number; title: string; start: string; end: string; kind: string; note: string };

export const KINDS = ["study", "deadline", "test", "recording", "reminder", "other"];

export const pad = (n: number) => String(n).padStart(2, "0");

export const dayKey = (d: Date) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;

export const fmt = (d: Date) => `${dayKey(d)}T${pad(d.getHours())}:${pad(d.getMinutes())}`;

/** Midnight at the start of the week (Monday) that contains `d`. */
export function mondayOf(d: Date): Date {
  const x = new Date(d);
  x.setHours(0, 0, 0, 0);
  x.setDate(x.getDate() - ((x.getDay() + 6) % 7));
  return x;
}

/** Today's range, for asking the core for today's blocks. */
export function todayRange(): { from: string; to: string } {
  const from = new Date();
  from.setHours(0, 0, 0, 0);
  const to = new Date(from);
  to.setDate(to.getDate() + 1);
  return { from: fmt(from), to: fmt(to) };
}
