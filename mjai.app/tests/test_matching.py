from collections import Counter
from pathlib import Path

from mjai.matching import Matching


def test_matching_init():
    user_ratings_map = {1: 1500_00.0, 2: 1500_00.0, 3: 1500_00.0, 4: 1300_00.0}
    path_map = {
        1: Path("path1.zip"),
        2: Path("path2.zip"),
        3: Path("path3.zip"),
        4: Path("path4.zip"),
    }
    matching = Matching(user_ratings_map, path_map)
    matching.match_count = Counter({1: 1, 2: 1, 3: 1, 4: 0})

    assert matching.get_target_player() == 4
    assert set(matching.get_new_match_tuple(4)) == set([4, 1, 2, 3])
