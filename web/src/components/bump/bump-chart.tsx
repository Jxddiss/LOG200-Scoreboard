// Rang des équipes choisies après chaque partie (bump chart) ; un nœud marque chaque nouvelle version de jar.

import { pointAt, smooth, type TRankPoint } from '@/lib/bump';
import { dayFormat, fullFormat, ordinal } from '@/lib/format';
import { pageUrl } from '@/lib/url';
import { cn } from '@/lib/utils';
import type { TGameRow } from '@/types/games.types';
import {
  Fragment,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type FC,
  type KeyboardEvent,
} from 'react';
import { BUCKET_PX, dateTicks, EXACT_PX, MARGIN, rankAt, vertices, type TVertex } from './bump.utils';

type Props = {
  columns: TGameRow[];
  series: Map<string, TRankPoint[]>;
  selected: string[];
  palette: Map<string, string>;
  versionOf: (team: string, build?: string) => string;
};

type THover = { col: number | null; team: string | null };

const muted = 'var(--muted)';

const BumpChart: FC<Props> = ({ columns, series, selected, palette, versionOf }) => {
  const plot = useRef<HTMLDivElement>(null);
  const tip = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 240, height: 200 });
  const [hover, setHover] = useState<THover>({ col: null, team: null });
  const [touch, setTouch] = useState(false);
  // Au doigt, un premier appui montre la partie, un second sur la même colonne l'ouvre.
  const armed = useRef<number | null>(null);

  // Un redimensionnement redessine sans rejouer la timeline.
  useLayoutEffect(() => {
    const node = plot.current!;
    const observer = new ResizeObserver(() => {
      const box = node.getBoundingClientRect();
      setSize({ width: Math.max(240, box.width), height: Math.max(200, box.height) });
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const geom = useMemo(() => {
    const { width, height } = size;
    const drawn = selected.filter((t) => series.get(t)?.length);
    if (!columns.length || !drawn.length) return null;

    const ranks = drawn.flatMap((t) => series.get(t)!.map((p) => p.rank));
    const top = Math.floor(Math.min(...ranks));
    const bottom = Math.ceil(Math.max(...ranks));
    const inner = { w: width - MARGIN.left - MARGIN.right, h: height - MARGIN.top - MARGIN.bottom };
    const x = (col: number) => MARGIN.left + (columns.length > 1 ? (col / (columns.length - 1)) * inner.w : inner.w / 2);
    const y = (rank: number) => MARGIN.top + (bottom > top ? ((rank - top) / (bottom - top)) * inner.h : inner.h / 2);
    const step = Math.ceil((bottom - top + 1) / Math.max(1, Math.floor(inner.h / 18)));
    const buckets = columns.length * EXACT_PX <= inner.w ? columns.length : Math.floor(inner.w / BUCKET_PX);
    const paths = new Map<string, TVertex[]>();
    for (const team of drawn) {
      const points = series.get(team)!;
      paths.set(team, vertices(points, smooth(points, columns.length, buckets), columns.length));
    }
    return { x, y, inner, width, height, top, bottom, step, drawn, paths };
  }, [columns, series, selected, size]);

  // Rang exact, pas lissé, meilleure d'abord.
  const standingsAt = (col: number) =>
    (geom?.drawn ?? [])
      .map((team) => ({ team, p: pointAt(series.get(team)!, col), before: pointAt(series.get(team)!, col - 1) }))
      .filter((r): r is { team: string; p: TRankPoint; before: TRankPoint | undefined } => Boolean(r.p))
      .sort((a, b) => a.p.rank - b.p.rank);

  const showAt = (col: number, team = hover.team) => {
    if (!geom) return hide();
    col = Math.max(0, Math.min(columns.length - 1, col));
    const rows = standingsAt(col);
    if (!rows.some((r) => r.team === team)) team = rows[0]?.team ?? null;
    setHover({ col, team });
  };

  const hide = () => {
    armed.current = null;
    setHover((h) => ({ ...h, col: null }));
  };

  const pointed = (e: { clientX: number; clientY: number }) => {
    const box = plot.current!.getBoundingClientRect();
    const px = e.clientX - box.left;
    const py = e.clientY - box.top;
    const col = Math.round(((px - MARGIN.left) / geom!.inner.w) * (columns.length - 1));
    const at = Math.max(0, Math.min(columns.length - 1, col));
    let team: string | null = null;
    let best = Infinity;
    for (const [t, path] of geom!.paths) {
      if (path[0][0] > at) continue;
      const distance = Math.abs(geom!.y(rankAt(path, at)) - py);
      if (distance < best) [team, best] = [t, distance];
    }
    return { col: at, team };
  };

  // Filtré sur l'équipe visée si elle joue la partie, sinon sur les équipes choisies.
  const openGame = (col: number) => {
    const g = columns[col];
    const team = hover.team && [g.red.team, g.black.team].includes(hover.team) ? hover.team : '';
    location.href = pageUrl('./', { teams: team, game: g.id });
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (!geom || hover.col === null) return;
    const page = Math.max(1, Math.round(columns.length / 10));
    const moves: Record<string, number> = {
      ArrowLeft: hover.col - 1,
      ArrowRight: hover.col + 1,
      PageUp: hover.col - page,
      PageDown: hover.col + page,
      Home: 0,
      End: columns.length - 1,
    };
    if (e.key in moves) showAt(moves[e.key]);
    else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      const rows = standingsAt(hover.col);
      const i = rows.findIndex((r) => r.team === hover.team) + (e.key === 'ArrowUp' ? -1 : 1);
      showAt(hover.col, rows[Math.max(0, Math.min(rows.length - 1, i))]?.team ?? null);
    } else if (e.key === 'Enter' || e.key === ' ') openGame(hover.col);
    else if (e.key === 'Escape') hide();
    else return;
    e.preventDefault();
  };

  // Point visé, recalé sur le dessin du moment : il a pu changer depuis.
  const col = geom && hover.col !== null ? Math.max(0, Math.min(columns.length - 1, hover.col)) : null;
  const rows = col === null ? [] : standingsAt(col);
  const team = col === null ? null : rows.some((r) => r.team === hover.team) ? hover.team : (rows[0]?.team ?? null);
  const game = col === null ? null : columns[col];
  const winner = game ? (game.red.all[1] >= game.black.all[1] ? game.red.team : game.black.team) : '';
  const loser = game ? (winner === game.red.team ? game.black.team : game.red.team) : '';
  const left = geom && col !== null ? geom.x(col) : 0;

  useLayoutEffect(() => {
    const node = tip.current;
    if (!node || !geom || col === null) return;
    const width = node.offsetWidth;
    node.style.left = `${left + 14 + width <= geom.width ? left + 14 : Math.max(0, left - 14 - width)}px`;
  });

  let summary: string;
  if (!geom) {
    summary = !selected.length ? 'Aucune équipe choisie.' : 'Aucune partie de ces équipes dans cette fenêtre.';
  } else {
    const from = dayFormat.format(new Date(columns[0].ts));
    const to = dayFormat.format(new Date(columns[columns.length - 1].ts));
    const parts = [`Rang de ${geom.drawn.length} équipes sur ${columns.length} parties, du ${from} au ${to}`];
    const teams = geom.drawn.filter((t) => !series.get(t)![0].bot);
    if (teams.length) {
      const rank = (t: string) => series.get(t)!.at(-1)!.rank;
      const leader = teams.reduce((a, b) => (rank(a) <= rank(b) ? a : b));
      parts.push(`en tête des équipes suivies : ${leader}, ${ordinal(rank(leader))}`);
    }
    summary = `${parts.join(' ; ')}.`;
  }

  const focus = rows.find((r) => r.team === team);
  const reading = focus ? `${team} ${ordinal(focus.p.rank)}, ${Math.round(focus.p.rating)} Elo, ` : '';
  const on = (t: string) => t === team;

  return (
    <figure className="bump-figure">
      <div
        ref={plot}
        id="bump-plot"
        className="bump-plot"
        role="slider"
        tabIndex={0}
        aria-valuemin={0}
        aria-valuemax={geom ? columns.length - 1 : undefined}
        aria-valuenow={col ?? undefined}
        aria-valuetext={game ? `${reading}${fullFormat.format(new Date(game.ts))}, ${winner} bat ${loser}` : undefined}
        aria-describedby="bump-summary"
        aria-label="Parties : flèches gauche et droite pour parcourir, haut et bas pour changer d'équipe, Entrée pour rejouer"
        onPointerMove={(e) => {
          if (!geom) return;
          setTouch(e.pointerType === 'touch');
          const at = pointed(e);
          showAt(at.col, at.team);
        }}
        onPointerDown={(e) => setTouch(e.pointerType === 'touch')}
        onPointerLeave={() => {
          if (document.activeElement !== plot.current) hide();
        }}
        onClick={(e) => {
          if (!geom) return;
          const at = pointed(e);
          if (touch && armed.current !== at.col) {
            armed.current = at.col;
            return showAt(at.col, at.team);
          }
          openGame(at.col);
        }}
        onFocus={() => geom && showAt(hover.col ?? columns.length - 1)}
        onBlur={hide}
        onKeyDown={handleKeyDown}
      >
        <svg
          id="bump-svg"
          className={cn(col !== null && team && 'focusing')}
          aria-hidden="true"
          viewBox={`0 0 ${size.width} ${size.height}`}
        >
          {geom && (
            <>
              <g className="bump-grid">
                {Array.from({ length: geom.bottom - geom.top + 1 }, (_, k) => {
                  const r = geom.top + k;
                  return (
                    <Fragment key={r}>
                      <line x1={MARGIN.left} x2={MARGIN.left + geom.inner.w} y1={geom.y(r)} y2={geom.y(r)} />
                      {k % geom.step === 0 && (
                        <text x={MARGIN.left - 8} y={geom.y(r)} className="rank">
                          {r}
                        </text>
                      )}
                    </Fragment>
                  );
                })}
                {dateTicks(columns, geom.inner.w).map((t) => (
                  <text key={t.col} x={geom.x(t.col)} y={geom.height - 8} className="date" textAnchor={t.anchor}>
                    {t.text}
                  </text>
                ))}
              </g>
              {/* Les lignes du haut du classement sont dessinées en dernier, au-dessus des autres. */}
              <g className="bump-lines">
                {[...geom.drawn].reverse().map((t) => {
                  const path = geom.paths.get(t)!;
                  const d = path.map(([c, rank], i) => `${i ? 'L' : 'M'}${geom.x(c)},${geom.y(rank)}`).join('');
                  return (
                    <path
                      key={t}
                      d={d}
                      className={cn(series.get(t)![0].bot && 'bot', on(t) && 'on')}
                      style={{ stroke: palette.get(t) ?? muted }}
                      data-team={t}
                    />
                  );
                })}
              </g>
              <g className="bump-nodes">
                {[...geom.drawn].reverse().map((t) => {
                  const points = series.get(t)!;
                  const path = geom.paths.get(t)!;
                  const fill = { fill: palette.get(t) ?? muted };
                  return (
                    <Fragment key={t}>
                      {/* Départ de branche dans la fenêtre. */}
                      {points[0].x > 0 && (
                        <circle
                          cx={geom.x(path[0][0])}
                          cy={geom.y(path[0][1])}
                          r={3}
                          className={cn(on(t) && 'on')}
                          style={fill}
                          data-team={t}
                        />
                      )}
                      {points
                        .filter((p) => p.version)
                        .map((p) => {
                          const cy = geom.y(rankAt(path, p.x));
                          return (
                            <Fragment key={p.x}>
                              <circle
                                cx={geom.x(p.x)}
                                cy={cy}
                                r={5}
                                className={cn('version', on(t) && 'on')}
                                style={fill}
                                data-team={t}
                              />
                              <text x={geom.x(p.x)} y={cy - 9} className={cn('version-label', on(t) && 'on')} data-team={t}>
                                {versionOf(t, p.build)}
                              </text>
                            </Fragment>
                          );
                        })}
                    </Fragment>
                  );
                })}
              </g>
              <g className="bump-labels">
                {[...geom.drawn].reverse().map((t) => {
                  const path = geom.paths.get(t)!;
                  return (
                    <g
                      key={t}
                      transform={`translate(${MARGIN.left + geom.inner.w + 8},${geom.y(path[path.length - 1][1])})`}
                      className={cn(on(t) && 'on')}
                      data-team={t}
                    >
                      <circle r={4} style={{ fill: palette.get(t) ?? muted }} />
                      <text x={8}>{t}</text>
                    </g>
                  );
                })}
              </g>
            </>
          )}
        </svg>
        <div
          id="bump-cursor"
          className="bump-cursor"
          hidden={col === null}
          style={geom ? { left: `${left}px`, top: `${MARGIN.top}px`, height: `${geom.inner.h}px` } : undefined}
        />
        <div ref={tip} id="bump-tip" className="bump-tip" aria-hidden="true" hidden={!game}>
          {game && (
            <>
              <p className="tip-when">
                {`${fullFormat.format(new Date(game.ts))} · partie ${col! + 1} sur ${columns.length}`}
              </p>
              <p className="tip-game">
                <strong>{winner}</strong>
                {` bat ${loser}`}
              </p>
              <ol className="tip-rows">
                {rows.map(({ team: t, p, before }) => {
                  const move = before && before.rank !== p.rank ? (p.rank < before.rank ? ' ▲' : ' ▼') : '';
                  const played = t === game.red.team || t === game.black.team;
                  return (
                    <li key={t} className={cn(t === team && 'on', played && 'played')}>
                      <span className="chip-dot" style={{ background: palette.get(t) ?? muted }} />
                      <span className="tip-rank">{`${ordinal(p.rank)}${move}`}</span>
                      <span className="tip-team">{[t, p.bot ? 'bot' : versionOf(t, p.build)].filter(Boolean).join(' ')}</span>
                      <span className="tip-elo">{Math.round(p.rating)} Elo</span>
                    </li>
                  );
                })}
              </ol>
              <p className="tip-hint">{touch ? 'Touchez encore pour rejouer la partie' : 'Clic ou Entrée : rejouer la partie'}</p>
            </>
          )}
        </div>
      </div>
      <figcaption id="bump-summary" className="bump-summary">
        {summary}
      </figcaption>
    </figure>
  );
};

export { BumpChart };
