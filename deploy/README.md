# Déployer le ladder

Le ladder et sa page tournent avec un `docker compose up -d`. Chaque jar joue dans un bac à sable gVisor sans réseau (voir "Isolation des joueurs" dans `docs/formats.md`).

## Prérequis

- Une machine avec Docker : Linux de préférence, Docker Desktop fonctionne aussi.
- Au moins 4 cœurs : chaque partie réserve un cœur par joueur (`cpuset_red`, `cpuset_black` dans `ladder.toml`).
- Pour jouer plusieurs parties en même temps, déclarer un `[[slot]]` par partie dans `ladder.toml`. Il faut compter 2 cœurs et 2 × `memory` de RAM par slot, plus 1 ou 2 cœurs pour Docker et les arbitres. Avec 8 cœurs, par exemple, 3 slots sur les cœurs 2 à 7.
- Le service `ladder` tourne en mode privilégié, parce qu'il héberge le Docker interne et gVisor qui isolent les jars. Le code des équipes, lui, n'a ni privilège ni réseau.

## Installation

1. Télécharger `isolation-scoreboard-deploy.zip` depuis la [dernière release](https://github.com/VianAtETS/LOG200-Scoreboard/releases/latest) et l'extraire.
2. Créer la config : `cp ladder.example.toml ladder.toml`, puis régler au moins la session et le temps par coup.
3. Lancer `docker compose up -d` : le ladder, l'API et les données sur `http://<machine>:8080/` (autre port : `WEB_PORT=80 docker compose up -d`).
4. La page : sur GitHub Pages (section plus bas), ou sur ce serveur avec `COMPOSE_PROFILES=web docker compose up -d`, à `http://<machine>:8080/`.

## Inscrire les équipes

Les équipes soumettent elles-mêmes leur jar (formulaire de la page, `curl` ou CI ; voir "Soumission des jars" dans `docs/formats.md`). Pour chaque équipe :

```sh
docker compose exec ladder isolation-ladder add-team eq07
```

La commande affiche le token de l'équipe, une seule fois : le lui transmettre en privé. Une équipe entre au classement dès que son premier jar a passé le smoke test ; inutile de redémarrer.

Pour que chaque push sur `main` soumette le jar, ajouter [`templates/ladder.yml`](https://github.com/VianAtETS/LOG200-Scoreboard/blob/main/templates/ladder.yml) au template du lab. Chaque équipe règle alors la variable `LADDER_URL` et le secret `LADDER_TOKEN` de son dépôt (voir "Depuis la CI" dans `docs/formats.md`).

## Au quotidien

| Pour… | Faire |
| --- | --- |
| Suivre les parties | `docker compose logs -f ladder` |
| Voir les soumissions | `jars/submissions.jsonl` (verdict et raison de chaque refus) |
| Retirer une équipe | supprimer sa ligne dans `jars/teams.toml` et son dossier `jars/<équipe>/` |
| Changer le temps par coup | `time_ms` dans `ladder.toml`, puis `docker compose restart ladder` ; les bots `niveau-N` suivent le temps annoncé |
| Mettre à jour | `docker compose pull && docker compose up -d` |
| Mettre à jour la page seule (profil `web`) | `docker compose pull web && docker compose up -d web` : le ladder continue |
| Fixer une version | `SCOREBOARD_TAG=v0.2.0 docker compose up -d` |
| Arrêter | `docker compose down` (les parties restent dans `data/`) |
| Arrêter tout de suite | `docker compose down -t 0` : la partie en cours est perdue (rejouée si c'est un match de tournoi) |

Redémarrer, mettre à jour ou arrêter n'interrompt aucune partie : le ladder n'en lance plus, finit celles en cours (quelques minutes, 15 au plus), puis s'arrête. Pendant ce temps, une nouvelle soumission reçoit un 503.
| Sauvegarder | copier `data/games.jsonl` |

Les journaux des joueurs vont dans `data/logs/`, les jars et les tokens dans `jars/`. La page ne sert ni l'un ni l'autre.

## Tournoi hebdomadaire

La section `[tournament]` de `ladder.toml` tire un tournoi chaque semaine (`weekly = "sun 12:00"`, heure de Montréal) entre les équipes qui ont un jar accepté : double élimination à 5 s par coup, jars figés au tirage. Le ladder le joue en priorité, puis reprend ses parties ; la page Tournoi montre le tableau. Un créneau manqué de moins de 2 h (ladder arrêté) est rattrapé au démarrage. Règles et fichiers : `docs/spec-tournoi-hebdo.md`.

| Pour… | Faire |
| --- | --- |
| Lancer un tournoi maintenant | `docker compose exec ladder isolation-ladder tournament start /srv/ladder.toml` |
| Débloquer un match suspendu (3 relances en échec) | `docker compose exec ladder isolation-ladder tournament resume /srv/ladder.toml tournoi-2026-11-08 W3` |
| Désactiver | retirer `weekly` de `[tournament]`, puis `docker compose restart ladder` |

La commande dépose une requête dans `jars/admin/`, que le ladder lit en moins de 10 s.

Un jar absent (`no_show`) ne perd pas tout de suite en tournoi : la partie est rejouée 30 s plus tard, et l'absence ne compte qu'au dernier essai (`retries`, 3 par défaut). Un jar figé a passé le smoke test ; s'il ne se connecte pas, c'est le plus souvent la machine qui rame.

### Surchauffe

Un tournoi charge plus la machine que le ladder : ses parties s'enchaînent sans `pause_s`, à 5 s par coup, sur tous les slots. Un CPU trop chaud baisse sa fréquence, les jars démarrent plus lentement que `connect_timeout_ms` et perdent par `no_show`. La section `[thermal]` de `ladder.toml` arrête de lancer des parties au-dessus de `pause_c` (90 °C par défaut) et reprend sous `resume_c` (80 °C). `docker compose logs ladder | grep thermal` montre le capteur trouvé au démarrage et chaque pause. Le conteneur est privilégié, il lit donc le `/sys` de l'hôte. Sans capteur (dans une VM, par exemple), le ladder l'annonce au démarrage et joue sans garde-fou.

## Nom de domaine et HTTPS

Hors du périmètre de ce compose : placer le reverse proxy de votre choix (Caddy, Nginx…) devant le port 8080.

Pour un pic de visiteurs (des centaines à la fois), un CDN comme Cloudflare absorbe la charge. Le serveur marque `/data/*` `Cache-Control: public, max-age=15`, le rythme de rafraîchissement de la page. Cloudflare ne met pas en cache les `.json` et `.jsonl` par défaut : ajouter une Cache Rule sur le chemin `/data/*`, "Eligible for cache", durée en cache de l'origine respectée. Le serveur sert alors chaque fichier une fois par 15 s et par point de présence, peu importe l'audience.

Attention à `/api/submit` : Cloudflare coupe une requête sans réponse après 100 s (erreur 524), alors qu'un smoke test attend la partie en cours et les soumissions précédentes, ce qui peut prendre plusieurs minutes. Le ladder poursuit quand même : un 524 veut dire "verdict inconnu", pas "refusé" (voir `jars/submissions.jsonl` ou la version affichée sur la page). Pour que la CI reçoive toujours le verdict, faire pointer son `LADDER_URL` vers un nom d'hôte non proxifié (nuage gris dans Cloudflare) qui mène au même serveur.

## Direct des parties

La page Direct (`direct.html`) montre chaque partie en cours coup par coup, et le replay suit une partie lancée depuis un badge "direct". Le ladder les diffuse par une WebSocket sur `/api/live`, en lecture seule et sans authentification (protocole dans `docs/formats.md`). La section `[live]` de `ladder.toml` l'ouvre, le `Caddyfile` la relaie déjà ; sans elle, la page affiche "Direct indisponible" et le reste fonctionne.

- Une ancienne `ladder.toml` n'a pas de section `[live]` : ajouter `[live]` puis `listen = "0.0.0.0:8092"`, et `docker compose restart ladder`.
- Avec Nginx Proxy Manager, cocher "Websockets Support", comme pour `/api/remote`.
- Le ladder accepte 512 spectateurs à la fois et ferme toute connexion au bout de 2 h (la page se reconnecte seule). Derrière Cloudflare, une règle de rate limiting sur le chemin `/api/live` (par exemple 20 requêtes par minute et par IP) empêche un seul client de prendre toutes les places ; le ladder ne voit que l'adresse du proxy.

## Équipes distantes (optionnel)

Une équipe peut jouer sans téléverser son jar : il tourne sur sa machine, et `isolation-remote` relaie ses coups par une WebSocket sur `/api/remote` (voir "Jouer à distance" dans `docs/formats.md`). Pour l'ouvrir, décommenter la section `[remote]` de `ladder.toml`, puis `docker compose restart ladder`. Le `Caddyfile` relaie déjà `/api/remote` ; rien n'est exposé de plus que le port 8080.

L'équipe utilise son token habituel et l'adresse de la page : `isolation-remote --url https://ladder.example.com -- java -jar Client-eq07.jar`. Ses parties portent le badge "distant", et la case "Vérifiées seulement" de la page les retire du classement.

- Le mode distant demande HTTPS (section précédente) : le token passe dans l'en-tête de la WebSocket. `isolation-remote` avertit l'équipe si l'adresse est en `http://`.
- Avec Nginx Proxy Manager, cocher "Websockets Support" sur l'hôte proxy du ladder : sans elle, l'upgrade de `/api/remote` n'est pas relayé et le client voit une erreur 400 ou 502.
- Cloudflare relaie les WebSockets, mais coupe une connexion inactive après 100 s. Le ladder envoie un ping toutes les 10 s : la connexion reste ouverte.
- La latence compte dans le temps par coup de l'équipe. Comme pour `LADDER_URL`, un nom d'hôte non proxifié évite le détour par Cloudflare.
- `docker compose logs ladder | grep distant` montre les connexions. Le diagnostic du relais de chaque partie est dans `data/logs/`.

## Page sur GitHub Pages

Au lieu du profil `web`, la page peut être publiée sur GitHub Pages : une modification de `web/` est alors en ligne à chaque push sur `main`, sans tag. Elle lit les parties et l'API sur ce serveur, qui doit donc être en HTTPS (section "Nom de domaine et HTTPS").

1. Dans le dépôt GitHub, Settings > Pages : source "GitHub Actions".
2. Settings > Secrets and variables > Actions > Variables : créer `SCOREBOARD_URL` avec l'adresse du serveur, par exemple `https://ladder.example.com` (sans `/` final).
3. Lancer le workflow `Pages` (onglet Actions, "Run workflow") ou pousser une modification de `web/`.

Le serveur accepte déjà les requêtes venant d'un autre site (en-têtes CORS du `Caddyfile`) : rien à changer de ce côté, hormis être à jour.

## Tester la page en local

L'image de la page tourne seule et lit n'importe quel serveur en HTTPS :

```sh
docker run --rm -p 8000:80 -e SCOREBOARD_SERVER=https://ladder.example.com ghcr.io/vianatets/log200-scoreboard-web
```

Puis ouvrir `http://localhost:8000/web/`.
