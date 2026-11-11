SELECT record FROM (SELECT seq, record FROM games ORDER BY seq DESC LIMIT :limit) ORDER BY seq
