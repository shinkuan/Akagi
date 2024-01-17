import os

import pytest

from mjai import MjaiPlayerClient, TimeoutExpired, Verification


def test_verify_tsumogiri_bot():
    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        submission_file = "./examples/tsumogiri.zip"
        p = MjaiPlayerClient(submission_file)
        with Verification(p) as v:
            v.verify_start_game_response()
            v.verify_start_kyoku_response()


def test_verify_timeout_bot():
    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        submission_file = "./examples/timeoutbot.zip"
        p = MjaiPlayerClient(submission_file, timeout=1)
        with pytest.raises(TimeoutExpired), Verification(p) as v:
            v.verify_start_game_response()
            v.verify_start_kyoku_response()


def test_verify_shanten_bot_http_server():
    if bool(os.getenv("SKIP_TEST_WITH_DOCKER")) is False:
        submission_file = "./examples/shanten.zip"
        p = MjaiPlayerClient(submission_file)
        with Verification(p) as v:
            v.verify_start_game_response()
            v.verify_start_kyoku_response()
