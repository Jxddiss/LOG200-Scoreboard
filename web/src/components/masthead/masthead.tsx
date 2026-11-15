import { pageUrl } from '@/lib/url';
import type { FC, PropsWithChildren } from 'react';
import { Logo } from './logo';
import { pages, type TPageKey } from './masthead.data';
import { VersionBadge } from './version-badge';

type Props = PropsWithChildren & {
  current: TPageKey;
  showVersion?: boolean;
};

const Masthead: FC<Props> = ({ current, showVersion = true, children }) => {
  return (
    <header className="masthead">
      <div className="brand">
        <Logo />
        <h1 className="title">
          Ladder Isolation{' '}
          <span className="title-meta">
            <span className="title-course">LOG200</span>
            {showVersion && <VersionBadge />}
          </span>
        </h1>
      </div>
      <nav className="pages" aria-label="Pages">
        {pages.map(({ key, label, href }) => (
          <a
            key={key}
            href={pageUrl(href)}
            aria-current={key === current ? 'page' : undefined}
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
