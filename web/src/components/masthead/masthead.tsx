import { pageUrl } from '@/lib/url';
import type { FC, PropsWithChildren } from 'react';
import { pages, type TPageKey } from './masthead.data';
import { VersionBadge } from './version-badge';

type Props = PropsWithChildren & {
  current: TPageKey;
  showVersion?: boolean;
};

const Masthead: FC<Props> = ({ current, showVersion = true, children }) => {
  return (
    <header className="masthead">
      <h1 className="title">
        Ladder Isolation <span className="title-course">LOG200</span>
        {showVersion && <VersionBadge />}
      </h1>
      <nav className="pages" aria-label="Pages">
        {pages.map(({ key, label, href }) => (
          <a
            key={key}
            href={href}
            aria-current={key === current ? 'page' : undefined}
            // Complété au clic, avec les choix du moment.
            onClick={(e) => (e.currentTarget.href = pageUrl(href))}
          >
            {label}
          </a>
        ))}
      </nav>
      {children && <div className="controls">{children}</div>}
    </header>
  );
};

export { Masthead };
