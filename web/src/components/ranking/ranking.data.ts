import type { TPaceStat } from '@/types/summary.types';

// Mesures des colonnes à bascule, dans l'ordre du clic : libellé et titre.
export const PACE_STATS: Record<TPaceStat, [string, string]> = {
  mean: ['moy.', 'Temps moyen par coup'],
  p50: ['méd.', 'Temps médian par coup'],
  p95: ['p95', '95e centile du temps par coup'],
  p99: ['p99', '99e centile du temps par coup'],
  max: ['max', 'Temps maximal par coup'],
};

export const RECORD_STATS = {
  all: ['V–D', 'Victoires et défaites, toutes versions'],
  build: ['V–D v.', 'Victoires et défaites, version actuelle'],
} as const;

export const ELO_STATS = {
  rating: ['Elo', 'Elo actuel'],
  peak: ['Peak', 'Elo maximal atteint'],
} as const;

export type TRecordStat = keyof typeof RECORD_STATS;
export type TEloStat = keyof typeof ELO_STATS;

// p99 au-delà de cette part du temps par coup : l'équipe frôle le timeout.
export const PACE_WARN = 0.8;

export const DEFAULT_STATS = { pace: 'p95', record: 'all', elo: 'rating' } as const;

export const statOf = <T extends string>(stats: Record<T, unknown>, value: string | null, fallback: T): T =>
  value !== null && value in stats ? (value as T) : fallback;

export const nextStat = <T extends string>(stats: Record<T, unknown>, current: T): T => {
  const keys = Object.keys(stats) as T[];
  return keys[(keys.indexOf(current) + 1) % keys.length];
};
