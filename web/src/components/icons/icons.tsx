// Icônes de la page, en traits : leur couleur et leur épaisseur viennent du CSS.
import type { FC, SVGProps } from 'react';

type Props = SVGProps<SVGSVGElement>;

const icon = (d: string, viewBox = '0 0 24 24') => {
  const Icon: FC<Props> = (props) => (
    <svg viewBox={viewBox} aria-hidden="true" {...props}>
      <path d={d} />
    </svg>
  );
  return Icon;
};

export const CloseIcon = icon('M6 6l12 12M18 6L6 18');
export const ChevronDownIcon = icon('M6 9l6 6 6-6');
export const FirstIcon = icon('M6 5v14M19 5l-9 7 9 7z');
export const PrevIcon = icon('M16 5l-9 7 9 7z');
export const PlayIcon = icon('M8 5l11 7-11 7z');
export const PauseIcon = icon('M8 5v14M16 5v14');
export const NextIcon = icon('M8 5l9 7-9 7z');
export const LastIcon = icon('M18 5v14M5 5l9 7-9 7z');

export const LiveIcon: FC<Props> = (props) => (
  <svg viewBox="0 0 24 24" aria-hidden="true" {...props}>
    <circle cx="12" cy="12" r="2.5" />
    <path d="M7.8 7.8a6 6 0 0 0 0 8.4M16.2 7.8a6 6 0 0 1 0 8.4M4.9 4.9a10 10 0 0 0 0 14.2M19.1 4.9a10 10 0 0 1 0 14.2" />
  </svg>
);

export const DuelIcon: FC<Props> = (props) => (
  <svg className="duel-icon" viewBox="0 0 20 16" aria-hidden="true" {...props}>
    <path className="duel-sword red" d="M3 1l12 12m-2-1 3 3m-5-1 4-4" />
    <path className="duel-sword black" d="M17 1L5 13m2-1-3 3m5-1-4-4" />
  </svg>
);
