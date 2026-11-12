import react from '@astrojs/react';
import { defineConfig } from 'astro/config';

// Ladder lu en dev : /api et /data sont relayés vers lui. SCOREBOARD_DEV_SERVER pour en viser un autre.
const DEV_SERVER = process.env.SCOREBOARD_DEV_SERVER ?? 'http://localhost:8080';

// https://astro.build/config
export default defineConfig({
  integrations: [react()],
  // Pages en fichiers (graphe.html, tournoi.html...) : les anciens liens restent valides.
  build: {
    format: 'file',
    // Chemins relatifs : la page se sert telle quelle sous /web/ (Docker) comme sous /<dépôt>/ (GitHub Pages).
    assetsPrefix: '.',
  },
  server: {
    port: 3000,
  },
  vite: {
    server: {
      proxy: {
        // Pas /api.html : seulement ce qui est sous /api/ et /data/.
        '^/api/': { target: DEV_SERVER, changeOrigin: true, ws: true },
        '^/data/': { target: DEV_SERVER, changeOrigin: true },
      },
    },
  },
});
