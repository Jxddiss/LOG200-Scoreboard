// Raison de la fin, du point de vue du perdant : liste compacte, puis phrase du replay.
export const REASONS: Record<string, string> = {
  no_moves: 'bloqué',
  timeout: 'temps',
  disconnect: 'déconnexion',
  protocol: 'coups invalides',
  no_show: 'absent',
};

export const ENDINGS: Record<string, (loser: string) => string> = {
  no_moves: (loser) => `${loser} n'a plus de coup`,
  timeout: (loser) => `${loser} a dépassé son temps`,
  disconnect: (loser) => `${loser} s'est déconnecté`,
  protocol: (loser) => `${loser} a joué trop de coups invalides`,
  no_show: (loser) => `${loser} ne s'est pas connecté`,
};

export const PLAY_MS = 500;
