use super::board::{Board, BoardState, Poll};
use super::result::GameResult;
use crate::agent::BatchAgent;
use crate::mjai::{Event, EventExt};
use std::collections::VecDeque;
use std::mem;

use anyhow::{ensure, Result};
use ndarray::prelude::*;

fn get_error_delta(game: &Game) -> Result<[i32; 4]> {
    let mut err_deltas = [0, 0, 0, 0];
    if game.last_index.player_id_idx == 0 {
        err_deltas = match game.board.oya {
            0 => [-12000, 4000, 4000, 4000],
            1 => [-8000, 4000, 2000, 2000],
            2 => [-8000, 2000, 4000, 2000],
            3 => [-8000, 2000, 2000, 4000],
            _ => [0, 0, 0, 0],
        };
    }
    if game.last_index.player_id_idx == 1 {
        err_deltas = match game.board.oya {
            0 => [4000, -8000, 2000, 2000],
            1 => [4000, -12000, 4000, 4000],
            2 => [2000, -8000, 4000, 2000],
            3 => [2000, -8000, 2000, 4000],
            _ => [0, 0, 0, 0],
        };
    }
    if game.last_index.player_id_idx == 2 {
        err_deltas = match game.board.oya {
            0 => [4000, 2000, -8000, 2000],
            1 => [2000, 4000, -8000, 2000],
            2 => [4000, 4000, -12000, 4000],
            3 => [2000, 2000, -8000, 4000],
            _ => [0, 0, 0, 0],
        };
    }
    if game.last_index.player_id_idx == 3 {
        err_deltas = match game.board.oya {
            0 => [4000, 2000, 2000, -8000],
            1 => [2000, 4000, 2000, -8000],
            2 => [2000, 2000, 4000, -8000],
            3 => [4000, 4000, 4000, -12000],
            _ => [0, 0, 0, 0],
        };
    }

    Ok(err_deltas)
}

pub struct BatchGame {
    /// 8 for hanchan and 4 for tonpuu
    pub length: u8,
    pub init_scores: [i32; 4],
}

#[derive(Clone, Copy, Default)]
pub struct Index {
    /// For `Game` to find a specific `Agent`.
    pub agent_idx: usize,
    /// For `Agent` to find a specific player ID accordingly.
    pub player_id_idx: usize,
}

#[derive(Default)]
struct Game {
    length: u8,
    seed: (u64, u64),
    indexes: [Index; 4],

    oracle_obs_versions: [Option<u32>; 4],
    invisible_state_cache: [Option<Array2<f32>>; 4],

    last_reactions: [EventExt; 4], // cached for poll phase
    last_index: Index,

    board: BoardState,
    kyoku: u8,
    honba: u8,
    kyotaku: u8,
    scores: [i32; 4],
    game_log: Vec<Vec<EventExt>>,

    kyoku_started: bool,
    ended: bool,
    /// Used in 西入 where the oya and another player get to 30000 at the same
    /// time, but the game continues because oya is not the top.
    ///
    /// As per [Tenhou's rule](https://tenhou.net/man/#RULE):
    ///
    /// > サドンデスルールは、30000点(供託未収)以上になった時点で終了、ただし親の
    /// > 連荘がある場合は連荘を優先する
    in_renchan: bool,
}

impl Game {
    fn poll(&mut self, agents: &mut [Box<dyn BatchAgent>]) -> Result<()> {
        if self.ended {
            return Ok(());
        }

        if !self.kyoku_started {
            if self.kyoku >= self.length + 4 // no 北入
                || self.kyoku >= self.length // in 西入
                    && !self.in_renchan // oya is not in renchan
                    && self.scores.iter().any(|&s| s >= 30000)
            {
                self.ended = true;
                return Ok(());
            }

            let mut next_board = Board {
                kyoku: self.kyoku,
                honba: self.honba,
                kyotaku: self.kyotaku,
                scores: self.scores,
                ..Default::default()
            };
            next_board.init_from_seed(self.seed);
            self.board = next_board.into_state();
            self.kyoku_started = true;
        }

        let reactions = mem::take(&mut self.last_reactions);

        let poll = self.board.poll(reactions)?;
        match poll {
            Poll::InGame => {
                let ctx = self.board.agent_context();
                for (player_id, state) in ctx.player_states.iter().enumerate() {
                    if !state.last_cans().can_act() {
                        continue;
                    }

                    let invisible_state = self.oracle_obs_versions[player_id]
                        .map(|ver| self.board.encode_oracle_obs(player_id as u8, ver));
                    self.invisible_state_cache[player_id] = invisible_state.clone();
                    let idx = self.indexes[player_id];
                    self.last_index = idx;
                    agents[idx.agent_idx].set_scene(
                        idx.player_id_idx,
                        ctx.log,
                        state,
                        invisible_state,
                    )?;
                }
            }

            Poll::End => {
                self.kyoku_started = false;
                self.in_renchan = false;

                for idx in &self.indexes {
                    // self.last_index = idx;
                    agents[idx.agent_idx].end_kyoku(idx.player_id_idx)?;
                }

                let kyoku_result = self.board.end();
                self.kyotaku = kyoku_result.kyotaku_left;
                self.scores = kyoku_result.scores;

                let logs = self.board.take_log();
                self.game_log.push(logs);

                let has_tobi = self.scores.iter().any(|&s| s < 0);
                if has_tobi {
                    self.ended = true;
                    return Ok(());
                }

                if kyoku_result.has_abortive_ryukyoku {
                    self.honba += 1;
                    return Ok(());
                }

                if !kyoku_result.can_renchan {
                    self.kyoku += 1;
                    if kyoku_result.has_hora {
                        self.honba = 0;
                    } else {
                        self.honba += 1;
                    }
                    return Ok(());
                }

                // renchan owari
                //
                // Conditions:
                // 1. can renchan
                // 2. is at all-last
                // 3. oya has at least 30000
                // 4. oya is the top
                let oya = kyoku_result.kyoku as usize % 4;
                if kyoku_result.kyoku >= self.length - 1 && self.scores[oya] >= 30000 {
                    let top = kyoku_result
                        .scores
                        .iter()
                        .enumerate()
                        .min_by_key(|&(_, &s)| -s)
                        .map(|(i, _)| i)
                        .unwrap();
                    if top == oya {
                        self.ended = true;
                        return Ok(());
                    }
                }

                // renchan
                self.in_renchan = true;
                self.honba += 1;
            }
        };

        Ok(())
    }

    fn commit(&mut self, agents: &mut [Box<dyn BatchAgent>]) -> Result<Option<GameResult>> {
        if self.ended {
            if self.kyotaku > 0 {
                *self.scores.iter_mut().min_by_key(|s| -**s).unwrap() += self.kyotaku as i32 * 1000;
            }

            let names = [
                agents[self.indexes[0].agent_idx].name(),
                agents[self.indexes[1].agent_idx].name(),
                agents[self.indexes[2].agent_idx].name(),
                agents[self.indexes[3].agent_idx].name(),
            ];
            let game_result = GameResult {
                names,
                scores: self.scores,
                seed: self.seed,
                game_log: mem::take(&mut self.game_log),
            };

            for idx in &self.indexes {
                // self.last_index = idx;
                agents[idx.agent_idx].end_game(idx.player_id_idx, &game_result)?;
            }
            return Ok(Some(game_result));
        }

        let ctx = self.board.agent_context();
        for (player_id, state) in ctx.player_states.iter().enumerate() {
            if !state.last_cans().can_act() {
                continue;
            }

            let invisible_state = self.invisible_state_cache[player_id].take();

            let idx = self.indexes[player_id];
            self.last_index = idx;
            self.last_reactions[player_id] = agents[idx.agent_idx].get_reaction(
                idx.player_id_idx,
                ctx.log,
                state,
                invisible_state,
            )?;
        }

        Ok(None)
    }
}

impl BatchGame {
    pub const fn tenhou_hanchan() -> Self {
        Self {
            length: 8,
            init_scores: [25000; 4],
        }
    }

    fn run_internal(
        &self,
        scores: [i32; 4],
        kyoku: u8,
        honba: u8,
        kyotaku: u8,
        agents: &mut [Box<dyn BatchAgent>],
        indexes: &[[Index; 4]],
        seeds: &[(u64, u64)],
    ) -> Result<Vec<GameResult>> {
        ensure!(!agents.is_empty());
        ensure!(!indexes.is_empty());
        ensure!(
            indexes.len() == seeds.len(),
            "expected `indexes.len() == seeds.len()`, got {} and {}",
            indexes.len(),
            seeds.len(),
        );

        let mut games = indexes
            .iter()
            .zip(seeds)
            .enumerate()
            .map(|(game_idx, (idxs, &seed))| {
                let mut oracle_obs_versions = [None; 4];
                for (i, idx) in idxs.iter().enumerate() {
                    // self.last_index = idx;
                    agents[idx.agent_idx].start_game(idx.player_id_idx)?;
                    oracle_obs_versions[i] = agents[idx.agent_idx].oracle_obs_version();
                }

                let game = Box::new(Game {
                    length: self.length,
                    seed,
                    indexes: *idxs,
                    scores: scores,
                    kyoku: kyoku,
                    honba: honba,
                    kyotaku: kyotaku,
                    oracle_obs_versions,
                    ..Default::default()
                });
                Ok((game_idx, game))
            })
            .collect::<Result<VecDeque<_>>>()?;

        let mut records = vec![GameResult::default(); games.len()];
        let mut to_remove = vec![];

        while !games.is_empty() {
            for (idx_for_rm, (game_idx, game)) in games.iter_mut().enumerate() {
                loop {
                    match game.poll(agents) {
                        Err(_err) => {
                            // Rescure game result
                            let err_msg = _err.to_string();
                            let err_deltas = get_error_delta(game)?;
                            println!("{}", err_msg);

                            let error_msg = "error".to_string();
                            game.board.add_log(EventExt {
                                event: Event::Ryukyoku {
                                    deltas: Some(err_deltas),
                                    reason: Some(error_msg),
                                },
                                meta: None,
                            });
                            game.board.add_log(EventExt {
                                event: Event::EndKyoku,
                                meta: None,
                            });
                            let logs = game.board.take_log();
                            game.game_log.push(logs);

                            let names = [
                                agents[game.indexes[0].agent_idx].name(),
                                agents[game.indexes[1].agent_idx].name(),
                                agents[game.indexes[2].agent_idx].name(),
                                agents[game.indexes[3].agent_idx].name(),
                            ];
                            let game_result = GameResult {
                                names,
                                scores: game.scores,
                                seed: game.seed,
                                game_log: mem::take(&mut game.game_log),
                            };
                            records[*game_idx] = game_result;

                            to_remove.push(idx_for_rm);
                            break;
                        }
                        _ => {}
                    }

                    if game.ended || game.kyoku_started {
                        break;
                    }
                }
            }
            for idx_for_rm in to_remove.drain(..).rev() {
                games.remove(idx_for_rm);
            }

            for (idx_for_rm, (game_idx, game)) in games.iter_mut().enumerate() {
                match game.commit(agents) {
                    Ok(Some(record)) => {
                        records[*game_idx] = record;
                        to_remove.push(idx_for_rm);
                    }
                    Err(_err) => {
                        // Rescure game result
                        let err_msg = _err.source().unwrap().to_string();
                        let err_deltas = get_error_delta(game)?;
                        println!("{}", err_msg);

                        let error_msg = "error".to_string();
                        game.board.add_log(EventExt {
                            event: Event::Ryukyoku {
                                deltas: Some(err_deltas),
                                reason: Some(error_msg),
                            },
                            meta: None,
                        });
                        game.board.add_log(EventExt {
                            event: Event::EndKyoku,
                            meta: None,
                        });
                        let logs = game.board.take_log();
                        game.game_log.push(logs);

                        let names = [
                            agents[game.indexes[0].agent_idx].name(),
                            agents[game.indexes[1].agent_idx].name(),
                            agents[game.indexes[2].agent_idx].name(),
                            agents[game.indexes[3].agent_idx].name(),
                        ];
                        let game_result = GameResult {
                            names,
                            scores: game.scores,
                            seed: game.seed,
                            game_log: mem::take(&mut game.game_log),
                        };
                        records[*game_idx] = game_result;

                        to_remove.push(idx_for_rm);
                        break;
                    }
                    _ => {}
                }
            }
            for idx_for_rm in to_remove.drain(..).rev() {
                games.remove(idx_for_rm);
            }
        }

        Ok(records)
    }

    pub fn run_continue(
        &self,
        scores: [i32; 4],
        kyoku: u8,
        honba: u8,
        kyotaku: u8,
        agents: &mut [Box<dyn BatchAgent>],
        indexes: &[[Index; 4]],
        seeds: &[(u64, u64)],
    ) -> Result<Vec<GameResult>> {
        self.run_internal(scores, kyoku, honba, kyotaku, agents, indexes, seeds)
    }

    pub fn run(
        &self,
        agents: &mut [Box<dyn BatchAgent>],
        indexes: &[[Index; 4]],
        seeds: &[(u64, u64)],
    ) -> Result<Vec<GameResult>> {
        let init_scores = [25000, 25000, 25000, 25000];
        let kyoku = 0;
        let honba = 0;
        let kyotaku = 0;

        self.run_internal(init_scores, kyoku, honba, kyotaku, agents, indexes, seeds)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::agent::Tsumogiri;

    #[test]
    fn tsumogiri() {
        let g = BatchGame::tenhou_hanchan();
        let mut agents = [
            Box::new(Tsumogiri::new_batched(&[0, 1, 2, 3]).unwrap()) as _,
            Box::new(Tsumogiri::new_batched(&[3, 2, 1, 0]).unwrap()) as _,
        ];
        let indexes = &[
            [
                Index {
                    agent_idx: 0,
                    player_id_idx: 0,
                },
                Index {
                    agent_idx: 0,
                    player_id_idx: 1,
                },
                Index {
                    agent_idx: 1,
                    player_id_idx: 1,
                },
                Index {
                    agent_idx: 1,
                    player_id_idx: 0,
                },
            ],
            [
                Index {
                    agent_idx: 1,
                    player_id_idx: 3,
                },
                Index {
                    agent_idx: 1,
                    player_id_idx: 2,
                },
                Index {
                    agent_idx: 0,
                    player_id_idx: 2,
                },
                Index {
                    agent_idx: 0,
                    player_id_idx: 3,
                },
            ],
        ];

        g.run(&mut agents, indexes, &[(1009, 0), (1021, 0)])
            .unwrap();
    }
}
