# API HTTP du ladder

Toutes les routes sont sous `/api/`. Dans le déploiement Docker, Caddy les relaie vers le ladder (voir [deploy/Caddyfile](../deploy/Caddyfile)) et ajoute `Access-Control-Allow-Origin: *` : une page servie ailleurs (GitHub Pages) peut les appeler. Le ladder sert les mêmes routes sur chaque adresse de `ladder.toml` (`listen` de `[submit]`, `[live]` et `[remote]`). Il faut donc au moins une de ces sections pour avoir l'API.

| Route | Accès | Rôle | Détail |
| --- | --- | --- | --- |
| `GET /api/health` | public | `ok`, pour la surveillance | |
| `GET /api/version` | public | version du ladder | |
| `PUT /api/submit` (ou `POST`) | token d'équipe | soumission d'un jar | [formats.md, Soumission des jars](formats.md#soumission-des-jars) |
| `GET /api/submit/{id}` | token d'équipe | verdict d'une soumission `?async` | [formats.md, Soumission des jars](formats.md#soumission-des-jars) |
| `GET /api/live` | public | parties en cours, en WebSocket | [formats.md, Direct](formats.md#direct-apilive) |
| `GET /api/remote` | token d'équipe | relais d'une équipe distante, en WebSocket | [formats.md, Jouer à distance](formats.md#jouer-à-distance) |
| `GET /api/games` | public | les 5000 dernières parties complètes, en JSONL | ci-dessous |
| `GET /api/games/{id}` | public | une partie complète, pour le replay | ci-dessous |
| `GET /api/stats/{nom}` | public | statistiques calculées dans la base | ci-dessous |

Une route inconnue répond 404. Si la base est illisible, les routes de lecture répondent 500 avec `base indisponible`, et le ladder écrit la cause sur sa sortie d'erreur.

## Dernières parties : `GET /api/games`

Les 5000 dernières parties, une par ligne, de la plus ancienne à la plus récente, au même format que `GET /api/games/{id}` (l'ancien `games.jsonl`). Le ladder les garde en mémoire et ne relit la base qu'une fois par minute au plus, peu importe le nombre d'appels. Si la base est illisible, il sert sa dernière copie.

```sh
curl https://<ladder>/api/games > recent.jsonl
```

| Code | Sens |
| --- | --- |
| 200 | `application/x-ndjson`, avec `Cache-Control: public, max-age=60`. Vide s'il n'y a aucune partie. |
| 500 | `base indisponible`, seulement si le ladder n'a encore aucune copie en mémoire. |

## Une partie : `GET /api/games/{id}`

La partie telle qu'elle a été enregistrée : l'objet résultat de l'arbitre, complété par le ladder (format dans [formats.md, Base des parties](formats.md#base-des-parties-stateladderdb)). L'`id` vient de `summary.json`, `teams/<équipe>.jsonl` ou `live.json`.

```sh
curl https://<ladder>/api/games/20261024T142103.512Z-eq07-eq12
```

| Code | Sens |
| --- | --- |
| 200 | `application/json`, avec `Cache-Control: public, max-age=31536000, immutable` : une partie finie ne change plus, un CDN peut la garder. |
| 404 | Aucune partie avec cet id (encore en cours, annulée, ou id inconnu). |

## Statistiques : `GET /api/stats/{nom}`

Chaque statistique est une requête SQL de `crates/isolation-ladder/src/sql/stats_*.sql`, calculée à chaque appel. Les réponses portent `Cache-Control: public, max-age=15`, le rythme de rafraîchissement de la page. Derrière un CDN, chaque statistique n'est donc recalculée qu'une fois toutes les 15 s (voir la Cache Rule de [deploy/README.md](../deploy/README.md#nom-de-domaine-et-https), à étendre à `/api/stats/*`).

Paramètre commun, facultatif :

- `session` : se limiter à une session (`?session=automne-2026`) ou à un tournoi, qui a sa propre session (`?session=tournoi-2026-11-08`). Sans lui, toutes les sessions sont comptées, mais sans les parties de tournoi, comme dans le classement « toutes sessions ».

Une session inconnue donne une liste vide, pas une erreur.

### `teams` : bilan par équipe

Classé par victoires, puis par nombre de parties. `losses` compte les défaites par raison (`no_moves`, `timeout`, `disconnect`, `protocol`, `no_show`, voir [formats.md, Objet résultat](formats.md#objet-résultat)).

```json
[
  { "team": "eq200", "games": 10375, "wins": 6965,
    "red": { "games": 5188, "wins": 3486 }, "black": { "games": 5187, "wins": 3479 },
    "losses": { "no_moves": 3368, "timeout": 33, "no_show": 7, "disconnect": 2 } }
]
```

### `head-to-head` : face-à-face

Une entrée par paire d'équipes qui se sont rencontrées, `a` avant `b` dans l'ordre alphabétique.

```json
[ { "a": "eq200", "b": "eq417", "games": 412, "a_wins": 230, "b_wins": 182 } ]
```

### `openings` : ouvertures et premiers coups

- `openings` : les 50 ouvertures imposées les plus jouées (demi-coups tirés par le ladder, voir `opening_plies`), avec les victoires du rouge.
- `first_moves` : pour chaque couleur, les 50 premiers coups clients les plus joués, avec les victoires de la couleur qui les a joués.

```json
{
  "openings": [ { "opening": ["G1-E3-I12", "F12-E11-H3"], "games": 34, "red_wins": 18 } ],
  "first_moves": {
    "red": [ { "move": "I1-F4-A7", "games": 210, "wins": 131 } ],
    "black": [ { "move": "D10-C10-I7", "games": 187, "wins": 90 } ]
  }
}
```

### `builds?team=<équipe>` : par version de jar

Le bilan de chaque build (16 premiers caractères hexadécimaux du sha256 du jar) de l'équipe, dans l'ordre où ils ont joué pour la première fois. Utile pour voir si une soumission a amélioré les résultats. Les parties sans build connu (bots de `[[team]]`) n'y sont pas.

```json
[
  { "build": "812198fd4ad7b97e", "games": 5551, "wins": 2472,
    "first": "2026-11-02T02:32:24.336Z", "last": "2026-11-08T23:41:14.721Z" }
]
```

| Code | Sens |
| --- | --- |
| 200 | `application/json`. |
| 400 | `builds` sans `team`, ou paramètres illisibles. |
| 404 | Statistique inconnue. |

## Requêtes libres

Pour une question que ces routes ne couvrent pas, la base s'interroge directement, sur le serveur ou sur une copie (voir [deploy/README.md](../deploy/README.md#au-quotidien)) :

```sh
sqlite3 -readonly state/ladder.db \
  "SELECT reason, count(*) FROM games WHERE session = 'automne-2026' GROUP BY reason"
```

Le schéma est dans `crates/isolation-ladder/src/sql/schema.sql` et décrit dans [formats.md, Base des parties](formats.md#base-des-parties-stateladderdb).
