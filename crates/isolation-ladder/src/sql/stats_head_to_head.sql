-- Victoires de chaque paire d'équipes, `a` avant `b` dans l'ordre alphabétique.
WITH scope AS (
    SELECT * FROM games
    WHERE (:session IS NULL AND tournament_id IS NULL) OR session = :session
),
pairs AS (
    SELECT min(red_team, black_team) AS a,
           max(red_team, black_team) AS b,
           count(*) AS games,
           sum(iif(winner = 'red', red_team, black_team) = min(red_team, black_team)) AS a_wins
    FROM scope
    WHERE red_team <> black_team
    GROUP BY a, b
)
SELECT json_group_array(
    json_object('a', a, 'b', b, 'games', games, 'a_wins', a_wins, 'b_wins', games - a_wins)
    ORDER BY a, b
)
FROM pairs
