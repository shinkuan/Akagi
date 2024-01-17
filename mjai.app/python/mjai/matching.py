import json
import random
import uuid
from collections import Counter
from pathlib import Path
from typing import Any, TypeAlias

from loguru import logger
from mjai.elo import update_multi_players_elo
from mjai.game import Simulator

UserId: TypeAlias = int
LogId: TypeAlias = str
DuplicateGame: TypeAlias = tuple[int, str, list[UserId]]


class Matching:
    def __init__(
        self,
        user_rating_map: dict[UserId, float],
        path_map: dict[UserId, Path],
    ):
        self.user_rating_map = user_rating_map
        self.path_map = path_map
        self.name_dict = {
            user_id: path.stem for user_id, path in path_map.items()
        }
        self.ratings = {
            user_id: rating / 100.0
            for user_id, rating in user_rating_map.items()
        }
        self.match_count: Counter = Counter(
            {user_id: 0 for user_id in self.ratings.keys()}
        )
        self.rows = [
            {
                self.name_dict[player_id]: self.ratings[player_id]
                for player_id in sorted(self.ratings.keys())
            }
        ]
        self.rows_only_updates = [
            {
                self.name_dict[player_id]: self.ratings[player_id]
                for player_id in sorted(self.ratings.keys())
            }
        ]

    def save_match_json(
        self, batch_date: str, matching_json: list[dict[str, Any]]
    ):
        matching_json_path = Path(f"./matching/{batch_date}.json")
        matching_json_path.parent.mkdir(parents=True, exist_ok=True)
        json.dump(matching_json, matching_json_path.open("w"))

    def save_elo_csv(self, batch_date: str, rows: list[dict[str, Any]]):
        csv_log = Path(f"./matching/{batch_date}.csv")
        player_names = sorted(self.name_dict.values())
        with csv_log.open("w") as f:
            header_line = ",".join(["log_id"] + player_names)
            f.write(header_line + "\n")
            for row in rows:
                f.write(row["log_id"])
                for player_name in player_names:
                    f.write(f",{row[player_name]:.2f}")
                f.write("\n")

    def save_elo_rating_json(self, batch_date: str):
        rating_dump_path = Path(f"./matching/{batch_date}.rating.json")
        rating_json = [
            {
                "path": str(self.path_map[user_id]),
                "id": user_id,
                "rating": (self.ratings[user_id] * 100.0),
            }
            for user_id in sorted(self.path_map.keys())
        ]
        json.dump(rating_json, rating_dump_path.open("w"))

    def match(
        self, batch_date: str
    ) -> tuple[list[dict[str, Any]], dict[str, Any]]:
        target_player_id = self.get_target_player()
        user_id_list = self.get_new_match_tuple(target_player_id)
        duplicate_game = self.get_duplicate_game(user_id_list)
        seed = self.get_random_seed()

        rows = []
        matching_detail = {}

        try:
            for dup_idx, log_id, user_id_list_ in duplicate_game:
                filepath_list = [
                    self.path_map[user_id] for user_id in user_id_list_
                ]
                logger.info(f"Start game {log_id} (dup_idx={dup_idx})")
                Simulator(
                    filepath_list,
                    logs_dir=f"./logs/{batch_date}/{log_id}",
                    seed=seed,
                ).run()

            matching_detail = self.collect_duplicate_game_result(
                duplicate_game, batch_date, seed
            )
            for match_user_id in user_id_list:
                self.match_count[match_user_id] += 1
            for log_idx, m in enumerate(matching_detail["matches"]):
                row = self.update_elo_ratings(m)
                rows.append(row)

        except Exception as e:
            logger.error(f"Unexpected error. {str(e)}")

        return rows, matching_detail

    def update_elo_ratings(self, match: dict[str, Any]) -> dict[str, Any]:
        before_match_ratings = [
            self.ratings[user_id] for user_id in match["user_ids"]
        ]
        after_match_ratings = update_multi_players_elo(
            before_match_ratings, match["ranks"]
        )

        # Update ratings
        for idx in range(4):
            self.ratings[match["user_ids"][idx]] = after_match_ratings[idx]

        row = {
            self.name_dict[user_id]: self.ratings[user_id]
            for user_id in sorted(self.ratings.keys())
        }
        row["log_id"] = match["log_id"]
        return row

    def get_random_seed(self) -> tuple[int, int]:
        seed_nonce = random.randint(1, 100000)
        seed_key = random.randint(1, 100000)
        return (seed_nonce, seed_key)

    def collect_duplicate_game_result(
        self,
        duplicate_game: list[DuplicateGame],
        batch_date: str,
        seed: tuple[int, int],
    ) -> dict[str, Any]:
        user_id_list = duplicate_game[0][2]

        summary_errors = []
        matches = []
        for dup_idx, log_id, dup_user_id_list in duplicate_game:
            summary_data = json.load(
                Path(f"./logs/{batch_date}/{log_id}/summary.json").open("r")
            )
            match_info = {
                "log_id": log_id,
                "user_ids": dup_user_id_list,
                "ranks": summary_data["rank"],
            }
            error_data = json.load(
                Path(f"./logs/{batch_date}/{log_id}/errors.json").open("r")
            )
            for error_info in error_data:
                if "player_id" in error_info:
                    summary_errors.append(
                        {
                            "duplication_index": dup_idx,
                            "player_id": error_info["player_id"],
                            "user_id": dup_user_id_list[
                                error_info["player_id"]
                            ],
                        }
                    )
            matches.append(match_info)

        return {
            "seed_value": seed,
            "users": user_id_list,
            "matches": matches,
            "errors": summary_errors,
        }

    def get_duplicate_game(
        self, user_ids: list[UserId]
    ) -> list[DuplicateGame]:
        return [
            (0, str(uuid.uuid4()), user_ids),
            (1, str(uuid.uuid4()), user_ids[1:] + user_ids[:1]),
            (2, str(uuid.uuid4()), user_ids[2:] + user_ids[:2]),
            (3, str(uuid.uuid4()), user_ids[3:] + user_ids[:3]),
        ]

    def get_target_player(self) -> UserId:
        # Get lowest frequency player
        return self.match_count.most_common()[-1][0]

    def get_new_match_tuple(self, target_user_id: UserId) -> list[UserId]:
        candidate_ids = list(self.ratings.keys())
        candidate_ids.remove(target_user_id)
        matched_user_ids = [target_user_id]
        base_rating = self.ratings[target_user_id]

        for _ in range(3):
            weights = [self.ratings[user_id] for user_id in candidate_ids]
            weights = [abs(w - base_rating) ** 2 / 10000.0 for w in weights]
            weights = [max(0.2, min(10.0, w)) ** -1 for w in weights]
            choice = random.choices(candidate_ids, k=1, weights=weights)
            choiced_user_id = choice[0]
            candidate_ids.remove(choiced_user_id)
            matched_user_ids.append(choiced_user_id)
        assert len(matched_user_ids) == 4
        return matched_user_ids
