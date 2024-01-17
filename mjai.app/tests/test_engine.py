import os

from mjai.engine import BaseMjaiLogEngine, DockerMjaiLogEngine
from mjai.mlibriichi.arena import Match

from mjai import MjaiPlayerClient


def test_game_four_new():
    env = Match(log_dir=None)
    assert env is not None

    agent1 = BaseMjaiLogEngine(name="0")
    agent2 = BaseMjaiLogEngine(name="1")
    agent3 = BaseMjaiLogEngine(name="2")
    agent4 = BaseMjaiLogEngine(name="3")

    try:
        ret = env.py_match(
            agent1,
            agent2,
            agent3,
            agent4,
            seed_start=(10000, 2000),
        )
        assert type(ret) is list
        assert len(ret) == 4
        for i in range(4):
            assert ret[i] in [0, 1, 2, 3]
    except RuntimeError:
        assert False

    assert True


def test_tsumogiri_player_with_docker_wrapper():
    env = Match()
    assert env is not None

    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        tsumogiri_player = MjaiPlayerClient("./examples/tsumogiri.zip")
        tsumogiri_player.launch_container(0)
        agent1 = DockerMjaiLogEngine(name="0", player=tsumogiri_player)
        agent2 = BaseMjaiLogEngine(name="1")
        agent3 = BaseMjaiLogEngine(name="2")
        agent4 = BaseMjaiLogEngine(name="3")

        try:
            ret = env.py_match(
                agent1,
                agent2,
                agent3,
                agent4,
                seed_start=(10000, 2000),
            )
            assert type(ret) is list
            assert len(ret) == 4
            for i in range(4):
                assert ret[i] in [0, 1, 2, 3]
        except RuntimeError as e:
            print("Error:\n" + str(e))
            assert False
        finally:
            tsumogiri_player.delete_container()


def test_multiple_players_with_docker_wrapper():
    env = Match(log_dir=None)
    assert env is not None

    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        players = [
            MjaiPlayerClient("./examples/tsumogiri.zip", port_num=28080),
            MjaiPlayerClient("./examples/tsumogiri.zip", port_num=28081),
            MjaiPlayerClient("./examples/tsumogiri.zip", port_num=28082),
            MjaiPlayerClient("./examples/shanten.zip", port_num=28083),
        ]
        for player_idx, player in enumerate(players):
            player.launch_container(player_idx)

        agents = [
            DockerMjaiLogEngine(name=str(player_idx), player=player)
            for player_idx, player in enumerate(players)
        ]

        exception_flag = False
        try:
            env.py_match(*agents, seed_start=(10000, 2000))

        except RuntimeError as e:
            print("Error:\n" + str(e))
            exception_flag = True

        finally:
            for player in players:
                player.delete_container()

        assert exception_flag is False
