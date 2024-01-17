import pytest
from mjai.elo import update_multi_players_elo


def test_update_multi_player_elo():
    ratings = [2140, 1500, 900, 1500]
    ranks = [1, 4, 3, 2]
    new_ratings = update_multi_players_elo(ratings, ranks)

    assert 2143.17 == pytest.approx(new_ratings[0], 0.1)
    assert 1404.96 == pytest.approx(new_ratings[1], 0.1)
    assert 959.63 == pytest.approx(new_ratings[2], 0.1)
    assert 1532.22 == pytest.approx(new_ratings[3], 0.1)
