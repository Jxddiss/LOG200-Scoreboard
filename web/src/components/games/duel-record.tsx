import { record } from '@/lib/elo';
import type { TGameRow } from '@/types/games.types';
import type { TTeamRating } from '@/types/summary.types';
import type { FC } from 'react';

type Props = {
  teams: string[];
  // Parties entre les deux équipes seulement.
  games: TGameRow[];
  ratings: TTeamRating[];
  versionOf: (team: string, build?: string) => string;
};

// Avec deux équipes choisies : bilan de la version actuelle de chacune contre l'autre.
const DuelRecord: FC<Props> = ({ teams, games, ratings, versionOf }) => {
  if (teams.length !== 2) return <p id="duel" className="graph-link" hidden />;

  const line = (team: string, opponent: string) => {
    const build = ratings.find((r) => r.team === team)?.build;
    const { wins, games: n } = record(games, team, opponent, build);
    const name = [team, versionOf(team, build)].filter(Boolean).join(' ');
    const score = n ? `${wins}–${n - wins} (${Math.round((100 * wins) / n)} %)` : 'aucune partie';
    return `${name} contre ${opponent} : ${score}`;
  };

  return (
    <p id="duel" className="graph-link" title="Version actuelle de chaque équipe, contre toutes les versions de l'autre">
      {`${line(teams[0], teams[1])} · ${line(teams[1], teams[0])}`}
    </p>
  );
};

export { DuelRecord };
