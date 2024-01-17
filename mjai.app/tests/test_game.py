from mjai.game import kyoku_to_zero_indexed_kyoku, to_rank


def test_to_rank():
    assert to_rank([25000, 25000, 25000, 25000]) == [1, 2, 3, 4]
    assert to_rank([2500, 60000, 5000, -900]) == [3, 1, 2, 4]


def test_kyoku_to_zero_indexed_kyoku():
    assert kyoku_to_zero_indexed_kyoku("E", 1) == 0
    assert kyoku_to_zero_indexed_kyoku("E", 2) == 1
    assert kyoku_to_zero_indexed_kyoku("E", 3) == 2
    assert kyoku_to_zero_indexed_kyoku("E", 4) == 3
    assert kyoku_to_zero_indexed_kyoku("S", 1) == 4
    assert kyoku_to_zero_indexed_kyoku("S", 4) == 7
    assert kyoku_to_zero_indexed_kyoku("W", 1) == 8
