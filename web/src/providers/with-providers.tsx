import type { ComponentType } from 'react';
import { QueryProvider } from './query-provider';

// Chaque page Astro monte une île React : elle apporte ses propres providers.
export function withProviders<P extends object>(Page: ComponentType<P>) {
  const WithProviders = (props: P) => (
    <QueryProvider>
      <Page {...props} />
    </QueryProvider>
  );
  WithProviders.displayName = `WithProviders(${Page.displayName ?? Page.name})`;
  return WithProviders;
}
