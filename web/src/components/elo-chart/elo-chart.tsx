// Courbe Elo de l'équipe choisie, comme un graphe de portefeuille : curseur, Elo, delta, résultat, date.

import { WindowRadio } from '@/components/controls/window-radio';
import { since, versionStarts } from '@/lib/elo';
import { WINDOW_OPTIONS, WINDOWS } from '@/lib/constants';
import { fullFormat, roundDelta, signed } from '@/lib/format';
import { cn } from '@/lib/utils';
import type { TGameRow, TRatingChange, TSide } from '@/types/games.types';
import type { TTeamRating } from '@/types/summary.types';
import { useRef, useState, type FC, type KeyboardEvent, type PointerEvent, type ReactNode } from 'react';

type TPoint = {
  rating: number;
  // Premier point d'une fenêtre qui ne commence pas au départ de l'équipe.
  before?: boolean;
  id?: string;
  ts?: string;
  delta?: number;
  game?: TGameRow;
  build?: string;
  version?: string;
  newVersion?: boolean;
};

type Props = {
  team: TTeamRating;
  rows: TGameRow[];
  rating: (side: TSide) => TRatingChange;
  versionOf: (team: string, build?: string) => string;
  window: string;
  // Heure de la dernière lecture : la fenêtre avance avec elle.
  now: number;
  onWindowChange: (value: string) => void;
  // Partie du replay : le curseur s'y pose.
  replayId: string | null;
  onOpen: (game: TGameRow) => void;
};

const EloChart: FC<Props> = ({ team: r, rows, rating, versionOf, window, now, onWindowChange, replayId, onOpen }) => {
  const plot = useRef<HTMLDivElement>(null);
  // Point visé à la souris ou au clavier ; sinon celui du replay, dès qu'il change.
  const [hover, setHover] = useState<number | null>(null);
  const [shownReplay, setShownReplay] = useState(replayId);
  if (shownReplay !== replayId) {
    setShownReplay(replayId);
    setHover(null);
  }

  const history: (TPoint & { ts: string })[] = rows.map((g) => {
    const side = g.red.team === r.team ? g.red : g.black;
    const [after, delta] = rating(side);
    return { id: g.id, ts: g.ts, rating: after, delta, game: g, build: side.build, version: versionOf(side.team, side.build) };
  });
  for (const k of versionStarts(history)) history[k].newVersion = true;
  const { start, points: recent } = since(history, now - (WINDOWS[window] ?? Infinity));

  const points: TPoint[] = recent.length ? [{ rating: start, before: recent.length < history.length }, ...recent] : [];
  const values = points.map((p) => p.rating);
  const min = Math.min(...values);
  const max = Math.max(...values);
  const last = points.length - 1;
  const replayIndex = points.findIndex((p) => p.id && p.id === replayId);
  const index = Math.max(0, Math.min(last, hover ?? (replayIndex > 0 ? replayIndex : last)));

  const pointAt = (e: PointerEvent) => {
    const box = plot.current!.getBoundingClientRect();
    return Math.round(((e.clientX - box.left) / box.width) * last);
  };

  const open = (i: number) => {
    const g = points[i]?.game;
    if (g) onOpen(g);
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    const moves: Record<string, number> = { ArrowLeft: index - 1, ArrowRight: index + 1, Home: 0, End: last };
    if (e.key in moves) setHover(Math.max(0, Math.min(last, moves[e.key])));
    else if (e.key === 'Enter' || e.key === ' ') open(index);
    else return;
    e.preventDefault();
  };

  let caption: ReactNode;
  let text = '';
  if (!points.length) {
    caption = (
      <>
        <strong>{Math.round(start)} Elo</strong> aucune partie dans cette fenêtre
      </>
    );
  } else {
    const p = points[index];
    text = `${Math.round(p.rating)} Elo`;
    if (p.game) {
      const color = p.game.red.team === r.team ? 'red' : 'black';
      const opponent = p.game[color === 'red' ? 'black' : 'red'].team;
      const d = roundDelta(p.delta!);
      const release = p.newVersion ? `, nouvelle version ${p.version}` : '';
      const context = ` ${p.game.winner === color ? 'victoire' : 'défaite'} contre ${opponent}, ${fullFormat.format(new Date(p.ts!))}${release}`;
      caption = (
        <>
          <strong>{Math.round(p.rating)} Elo</strong>
          {d ? (
            <>
              {' '}
              <span className={cn('delta', d > 0 ? 'up' : 'down')}>{signed(d)}</span>
            </>
          ) : null}
          <span>{context}</span>
        </>
      );
      text += `${d ? ` (${signed(d)})` : ''},${context}`;
    } else {
      const context = p.before ? ' début de la fenêtre' : ` départ de ${r.team}`;
      caption = (
        <>
          <strong>{Math.round(p.rating)} Elo</strong>
          <span>{context}</span>
        </>
      );
      text += `,${context}`;
    }
  }

  const bottom = max - min || 1;
  const line = values.map((v, i) => `${i},${max - v}`).join(' ');
  const x = last > 0 ? `${(100 * index) / last}%` : '0%';
  const y = `${(100 * (max - (points[index]?.rating ?? 0))) / (max - min || 1)}%`;

  return (
    <figure id="elo-chart" className="elo-chart">
      <figcaption id="elo-caption" className="elo-caption">
        {caption}
      </figcaption>
      <div
        ref={plot}
        id="elo-plot"
        className="elo-plot"
        role="slider"
        tabIndex={0}
        aria-valuemin={0}
        aria-valuemax={last}
        aria-valuenow={points.length ? index : undefined}
        aria-valuetext={text || undefined}
        aria-label="Parties de l'équipe : flèches pour parcourir, Entrée pour rejouer"
        hidden={!points.length}
        onPointerMove={(e) => setHover(pointAt(e))}
        onPointerDown={(e) => setHover(pointAt(e))}
        onPointerLeave={() => setHover(null)}
        // Le point affiché, pas la position du clic : `click` arrondit clientX et peut tomber sur le voisin.
        onClick={() => open(index)}
        onKeyDown={handleKeyDown}
      >
        <svg id="elo-line" aria-hidden="true" preserveAspectRatio="none" viewBox={points.length ? `0 0 ${last} ${bottom}` : undefined}>
          {points.length > 0 && (
            <>
              <polygon points={`0,${bottom} ${line} ${last},${bottom}`} />
              <polyline points={line} />
            </>
          )}
        </svg>
        <div id="elo-versions" aria-hidden="true">
          {points.map((p, i) => {
            if (!p.newVersion) return null;
            const left = (i - 1) / last;
            return (
              <div
                key={p.id}
                className={cn('elo-version', left > 0.9 && 'end')}
                data-label={p.version}
                style={{ left: `${100 * left}%` }}
              />
            );
          })}
        </div>
        <div id="elo-cursor" className="elo-cursor" style={{ left: x }} />
        <div id="elo-dot" className="elo-dot" style={{ left: x, top: y }} />
      </div>
      <WindowRadio id="elo-window" name="window" label="Fenêtre de la courbe" value={window} options={WINDOW_OPTIONS} onChange={onWindowChange} />
    </figure>
  );
};

export { EloChart };
