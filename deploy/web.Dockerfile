# Image de la page seule, pour l'heberger soi-meme ou la tester en local contre un autre serveur.
# Construite depuis la racine du depot : docker build -f deploy/web.Dockerfile .

# Site statique (Astro) : le meme pour toutes les plateformes, construit une fois sur celle du build.
FROM --platform=$BUILDPLATFORM node:24-alpine AS build
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web ./
RUN npm run build

FROM caddy:2.11.7-alpine
COPY --from=build /web/dist /srv/web
COPY --chmod=755 deploy/web-entrypoint.sh /usr/local/bin/
ENTRYPOINT ["web-entrypoint.sh"]
