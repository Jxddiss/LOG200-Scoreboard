import { cn } from '@/lib/utils';
import type { FC } from 'react';

type Props = {
  id?: string;
  name: string;
  label: string;
  value: string;
  options: { value: string; label: string }[];
  onChange: (value: string) => void;
  hidden?: boolean;
  className?: string;
};

// Boutons radio en pastilles : fenêtre des graphes, parties montrées.
const WindowRadio: FC<Props> = ({ id, name, label, value, options, onChange, hidden, className }) => {
  return (
    <div id={id} className={cn('elo-window', className)} role="radiogroup" aria-label={label} hidden={hidden}>
      {options.map((o) => (
        <label key={o.value}>
          <input
            type="radio"
            name={name}
            value={o.value}
            checked={o.value === value}
            onChange={() => onChange(o.value)}
          />
          {o.label}
        </label>
      ))}
    </div>
  );
};

export { WindowRadio };
