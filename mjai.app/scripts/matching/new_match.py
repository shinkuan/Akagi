import argparse
import json
from pathlib import Path

from loguru import logger
from mjai.matching import Matching, UserId


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "-i", "--input", type=Path, help="init_ratings JSON file"
    )
    parser.add_argument("-b", "--batch", type=str, default="2023/07/16")
    parser.add_argument("-n", "--num_game", type=int, default=100)
    return parser.parse_args()


class User:
    def __init__(
        self, submission_path: str | Path, user_id: int, init_rating: int
    ):
        self.user_id = user_id
        self.submission_path = Path(submission_path)
        self.init_rating = init_rating  # rating point before matching


def get_users(input_json_path: Path) -> tuple[dict[UserId, float], list[User]]:
    json_data = json.load(input_json_path.open("r"))
    user_ratings_map = {
        user_data["id"]: user_data["rating"] for user_data in json_data
    }
    submissions: list[User] = [
        User(user_data["path"], user_data["id"], user_data["rating"])
        for user_data in json_data
    ]
    return user_ratings_map, submissions


def main():
    args = parse_args()
    batch_date = args.batch
    matching_json_path = Path(f"./matching/{batch_date}.json")
    matching_json_path.parent.mkdir(parents=True, exist_ok=True)
    matching_json = []

    matching_log = Path(f"./matching/{batch_date}.log")
    matching_log.parent.mkdir(parents=True, exist_ok=True)
    logger.add(matching_log, level="DEBUG")

    user_ratings_map, submissions = get_users(args.input)
    path_map = {sub.user_id: sub.submission_path for sub in submissions}
    matching = Matching(user_ratings_map, path_map)

    rows = []
    n_matches = args.num_game
    for _ in range(n_matches):
        match_rows, matching_detail = matching.match(batch_date)
        rows += match_rows

        matching_json.append(matching_detail)
        matching.save_match_json(batch_date, matching_json)
        matching.save_elo_csv(batch_date, rows)
        matching.save_elo_rating_json(batch_date)


if __name__ == "__main__":
    main()
