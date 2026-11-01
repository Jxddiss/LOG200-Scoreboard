# Image de la page seule, pour l'heberger soi-meme ou la tester en local contre un autre serveur.
# Construite depuis la racine du depot : docker build -f deploy/web.Dockerfile .
FROM caddy:2.11.7-alpine
COPY web /srv/web
COPY --chmod=755 deploy/web-entrypoint.sh /usr/local/bin/
ENTRYPOINT ["web-entrypoint.sh"]
