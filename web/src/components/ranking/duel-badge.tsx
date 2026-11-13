import { DuelIcon } from '@/components/icons/icons';
import { percent } from '@/lib/predict';
import { cn } from '@/lib/utils';
import type { TColor } from '@/types/games.types';
import type { FC } from 'react';

type Props = {
  opponent: string;
  color: TColor;
  // L'adversaire a une ligne au classement : le badge y mène.
  listed: boolean;
  chance: number | null;
};

const DuelBadge: FC<Props> = ({ opponent, color, listed, chance }) => {
  const odds = chance === null ? '' : `, ${percent(chance)} % de chances de gagner selon le modèle`;
  const title = `Partie en cours contre ${opponent}${odds}`;
  const body = (
    <>
      <DuelIcon />
      <span className="duel-name">{opponent}</span>
      {chance !== null && <span className="duel-prob">{percent(chance)} %</span>}
    </>
  );

  if (!listed) {
    return (
      <span className={cn('duel-badge', color)} title={title}>
        {body}
      </span>
    );
  }

  return (
    <button
      className={cn('duel-badge', color)}
      type="button"
      title={title}
      aria-label={`${title}, voir sa ligne`}
      data-opponent={opponent}
    >
      {body}
    </button>
  );
};

export { DuelBadge };
