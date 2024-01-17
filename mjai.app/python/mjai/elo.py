mean_elo = 1500
elo_width = 1200
k_factor = 8


def update_multi_players_elo(elo_ratings, ranks):
    ratings = elo_ratings.copy()
    ranks_idx = [rank_idx for _, rank_idx in sorted(zip(ranks, range(4)))]
    for win_idx, lose_idx in [
        (0, 1),
        (0, 2),
        (0, 3),
        (1, 2),
        (1, 3),
        (2, 3),
    ]:
        winner_elo, loser_elo = update_two_players_elo(
            ratings[ranks_idx[win_idx]], ratings[ranks_idx[lose_idx]]
        )
        ratings[ranks_idx[win_idx]], ratings[ranks_idx[lose_idx]] = (
            winner_elo,
            loser_elo,
        )
    return ratings


def update_two_players_elo(winner_elo, loser_elo):
    expected_win = expected_result(winner_elo, loser_elo)
    change_in_elo = k_factor * (1 - expected_win)
    winner_elo += change_in_elo
    loser_elo -= change_in_elo
    return winner_elo, loser_elo


def expected_result(elo_a, elo_b):
    expect_a = 1.0 / (1 + 10 ** ((elo_b - elo_a) / elo_width))
    return expect_a
