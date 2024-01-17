def test_import():
    import mjai.mlibriichi.arena

    assert mjai.mlibriichi.arena
    assert "py_match" in dir(mjai.mlibriichi.arena.Match)
