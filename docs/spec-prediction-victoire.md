# Probabilité de victoire du scoreboard

Le scoreboard affiche, pour un duel, la probabilité que chaque équipe gagne : sur le badge des parties en cours et sur les matchs à venir du tableau de tournoi. Elle n'influence ni l'appariement du ladder ni le tirage du tournoi. Format de `model.json` : [formats.md](formats.md).

## Modèle

P(rouge gagne) = σ(s_rouge − s_noir + c), un modèle Bradley–Terry ajusté en régression logistique.

- `s` : force par équipe (clé `team`, comme l'Elo). `c` : avantage du rouge, global.
- Ajustement par Newton avec pénalité L2 sur `s` (une équipe invaincue garde une force finie), écrit à la main dans `src/bin/ladder/model.rs`, sans crate.
- Les parties les plus anciennes pèsent moins : le poids est divisé par deux tous les 500 parties.
- Parties retenues : vérifiées (aucun côté distant) et de tournoi, bots et ancres inclus comme adversaires. Les fins `no_show`, `disconnect` et `protocol` sont écartées, `no_moves` et `timeout` gardées.
- Variables : force par équipe et couleur seulement. Le profil `move_ms` d'une équipe est absorbé par sa force. Le budget `time_ms` ne varie pas assez, et l'ouverture est trop éparse.
- Paramètres (constantes de `model.rs`, à régler sur des données réelles) : pénalité 1, demi-vie 500 parties, ajustement toutes les 25 parties, 10 parties minimales par équipe, validation sur les 20 % les plus récentes (30 parties au moins).

## Validation et affichage

- À chaque ajustement, les 20 % de parties les plus récentes (1000 au plus) sont notées (Brier) une à une par un modèle ajusté sur ce qui les précède, réajusté toutes les 25 parties, contre l'`expected()` de l'Elo d'avant partie. Un modèle figé serait injustement pénalisé : l'Elo, lui, se met à jour partie par partie. S'il ne le bat pas, ou si la validation a moins de 30 parties, `usable` vaut faux et la page n'affiche aucune probabilité. Les métriques sont toujours publiées, avec l'historique des 20 derniers ajustements. Le Brier des parties de tournoi est donné à part.
- Une équipe avec moins de 10 parties : la page n'affiche rien.
- Badge de duel (`web/app.js`) : probabilité de l'équipe de la ligne avec sa couleur réelle.
- Tableau de tournoi (`web/tournoi.js`) : probabilité par manche des matchs à venir, moyenne des deux couleurs, dans la case du score tant que le match n'a pas commencé.
- Le web calcule σ lui-même (`web/predict.js`) à partir de `model.json`.

## Limites assumées

- Le budget de calcul mélange tournoi (5000 ms) et ladder : une seule force par équipe les confond. Le Brier des parties de tournoi, publié à part, montre l'écart.
- Le drapeau de threads n'est pas dans `games.jsonl` : la capacité de calcul n'est pas une variable.
- Pas de journal de prédictions partie par partie : le rejeu hors échantillon à chaque ajustement tient lieu de mesure.

## Decisions Log

| ID | Topic | Decision | Rationale | Source | Date |
|---|---|---|---|---|---|
| D1 | Objectif | Affichage seulement, aucun effet sur l'appariement | `Scheduler::next_pair` reste intact, risque faible | Interview | 2026-10-31 |
| D2 | Où ça tourne | Rust ajuste et publie `model.json`, le web calcule σ | Rust a tout l'historique ; le web ne voit ni `opening` ni `move_ms` ; aucun crate à ajouter | Interview | 2026-10-31 |
| D3 | Force | Un paramètre par équipe (Bradley–Terry classique), sans l'Elo | Choix de l'utilisateur, contre la recommandation (écart d'Elo) ; évite un Elo redondant | Interview | 2026-10-31 |
| D4 | Démarrage à froid | Ridge et seuil de parties, rien d'affiché sous le seuil | Pas de mélange de deux modèles | Interview | 2026-10-31 |
| D5 | Variables v1 | Couleur seulement | `move_ms` absorbé par la force d'équipe, `time_ms` constant, ouverture trop éparse | Interview | 2026-10-31 |
| D6 | Parties retenues | Vérifiées et tournoi, bots inclus | Choix de l'utilisateur, contre la recommandation (hors tournoi) ; plus de données et de signal de couleur | Interview | 2026-10-31 |
| D7 | Budget mixte | Accepté, consigné, Brier de tournoi publié à part | Une force unique confond les budgets ; la validation expose le biais | Interview | 2026-10-31 |
| D8 | Granularité | Par équipe, pondération récente | Aligné sur l'Elo, absorbe les changements de build | Interview | 2026-10-31 |
| D9 | Garde-fou | `usable` faux si le Brier ne bat pas l'Elo hors échantillon | Un modèle moins bon que l'Elo existant n'apporte rien | Interview | 2026-10-31 |
| D10 | Affichage | Badge de duel en direct et tableau de tournoi | Amende `spec-tournoi-hebdo.md` (prédictions hors périmètre) | Interview | 2026-10-31 |
| D11 | Journal | Aucun ; rejeu hors échantillon à chaque ajustement | Pas de changement de schéma ni de fichier | Interview | 2026-10-31 |
| D12 | Cadence | Au démarrage puis toutes les 25 parties, écriture atomique | Coût faible, affichage stable | Interview | 2026-10-31 |
| D13 | Red-team | Sautée par l'utilisateur | Séparation parfaite et graphe déconnecté couverts par les tests | Interview | 2026-10-31 |

## Dependency Graph & Implementation Order

```
Standings (expected, historique)   GameRecord
              │                        │
              └──────────┬─────────────┘
                         ▼
        1. model.rs : ajustement Newton + ridge + poids
                         ▼
        2. validation chronologique, Brier modèle contre Elo, garde-fou `usable`
                         ▼
        3. Standings : alimente le modèle, publie au démarrage puis toutes les 25 parties
                         ▼
        4. web/predict.js : σ(Δs ± c), seuil de parties, `usable`
                  ┌──────┴──────┐
                  ▼             ▼
        5a. badge de duel   5b. tableau de tournoi
                  └──────┬──────┘
                         ▼
        6. docs : formats.md, spec-tournoi-hebdo.md
```
