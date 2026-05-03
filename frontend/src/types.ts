// Mirrors backend schema. Source: frontend/README.md.

export type MjaiEvent =
  | { type: 'start_game'; names: string[]; kyoku_first?: number; aka_flag?: boolean; id?: number; num_players?: number }
  | { type: 'start_kyoku'; bakaze: string; dora_marker: string; kyoku: number; honba: number; kyotaku: number; oya: number; scores: number[]; tehais: string[][]; num_players?: number }
  | { type: 'tsumo'; actor: number; pai: string }
  | { type: 'dahai'; actor: number; pai: string; tsumogiri: boolean }
  | { type: 'chi'; actor: number; target: number; pai: string; consumed: [string, string] }
  | { type: 'pon'; actor: number; target: number; pai: string; consumed: [string, string] }
  | { type: 'daiminkan'; actor: number; target: number; pai: string; consumed: [string, string, string] }
  | { type: 'kakan'; actor: number; pai: string; consumed: [string, string, string] }
  | { type: 'ankan'; actor: number; consumed: [string, string, string, string] }
  | { type: 'dora'; dora_marker: string }
  | { type: 'reach'; actor: number; pai?: string }
  | { type: 'reach_accepted'; actor: number }
  | { type: 'hora'; actor: number; target: number; deltas?: number[]; ura_markers?: string[] }
  | { type: 'ryukyoku'; deltas?: number[] }
  | { type: 'kita'; actor: number; pai?: string }
  | { type: 'end_kyoku' }
  | { type: 'end_game' }
  | { type: 'none' }

export type BotResponse = MjaiEvent & { meta?: Record<string, unknown> }

/** Bot-driven custom display payload, attached on `meta.show`.
 *  Schema is intentionally generic so a single tile can render top-N
 *  actions, opponent reads, yaku breakdowns, etc. — bots decide the
 *  semantics and formatting. */
export type ShowItem = {
  /** Primary text on the row. */
  label?: string
  /** mjai tile strings; converted to mahgen via `mjaiToMahgen`. */
  pais?: string[]
  /** Raw mahgen DSL string. Wins over `pais` if both are set. */
  tiles?: string
  /** Right-side text (any format — e.g. "85.42%", "+12000"). */
  value?: string
  /** Hex accent color (e.g. "#00ff80") — applied as left bar + faint row tint. */
  color?: string
  /** Small subtitle under `label`. */
  note?: string
}

export type ShowMeta = {
  /** Optional title; falls back to the tile's default title. */
  title?: string
  items: ShowItem[]
}

export type BotStatus =
  | { state: 'idle' }
  | { state: 'loading'; bot: string; stage: 'syncing_deps' | 'spawning' }
  | { state: 'ready'; bot: string; actor_id: number }
  | { state: 'error'; bot: string; error: string }
  | { state: 'stopped'; bot: string }

export type CaptureKind = 'mitm' | 'chromium'

export type CaptureStatus =
  | { state: 'stopped' }
  | { state: 'starting'; kind: CaptureKind; descriptor: string }
  | { state: 'running'; kind: CaptureKind; descriptor: string }
  | { state: 'error'; kind: CaptureKind; descriptor?: string; error: string }

export type Notification = {
  level: 'info' | 'success' | 'warn' | 'error'
  title: string
  body?: string
  sticky: boolean
  id?: string
}

export type CaptureMode = 'mitm' | 'chromium'

export type ChromiumConfig = {
  executable: string
  user_data_dir: string
  start_url: string
  cft_channel: string
  force_cft: boolean
  extra_args: string[]
}

export type CaptureConfig = {
  mode: CaptureMode
  chromium: ChromiumConfig
}

export type DetectedBrowser = {
  kind: 'chrome' | 'edge' | 'brave' | 'chromium' | 'chrome_for_testing'
  path: string
}

/// Bridge selector (the runtime kind, not the history-record tag — those
/// share names but the schema enum carries extra archive-only variants).
/// Mirrors `src/config/platform.rs::Platform` (`#[derive(Serialize)]` →
/// PascalCase JSON: `"Majsoul"`, `"Tenhou"`).
export type PlatformKind = 'Majsoul' | 'Tenhou'

export type AppConfig = {
  general: { language: string; first_run_completed: boolean }
  logging: { dir: string; level: string; all_level: string }
  platform: { kind: PlatformKind }
  proxy: { enabled: boolean; addr: string; ca_dir: string }
  bot: { enabled: boolean; active_4p: string; active_3p: string; auto_sync: boolean; dir: string }
  capture: CaptureConfig
}

export type FieldKind = 'string' | 'bool' | 'int' | 'float' | 'enum'

export type FieldSpec = {
  type: FieldKind
  label: string
  default: unknown
  help?: string
  secret?: boolean
  min?: number
  max?: number
  step?: number
  choices?: string[]
}

export type Manifest = {
  manifest_version: number
  bot: {
    name: string
    display?: string
    description?: string
    version?: string
    /** Game modes this bot can play. Backend defaults to `["4p"]` when absent. */
    supported_modes: string[]
  }
  source?: { type: 'github_release'; repo: string; asset_glob?: string }
  settings: Record<string, FieldSpec>
}

export type BotInfo = {
  name: string
  dir: string
  has_pyproject: boolean
  manifest?: Manifest
}

export type BotSettings = {
  manifest: Manifest
  values: Record<string, unknown>
}

export type Snapshot = {
  config: AppConfig
  bot_status: BotStatus
  capture_status: CaptureStatus
  log_dir: string
}

export type WaitInfo = { tile: string; left: number; agari_rate: number | null }

export type ImproveEntry = { draw: string; widened_waits: WaitInfo[]; widened_total: number }

export type Hand13Result = {
  shanten: number
  waits: WaitInfo[]
  waits_total: number
  next_shanten_waits_count: { [tileIdx: number]: number }
  avg_next_shanten_waits: number
  mixed_waits_score: number
  avg_agari_rate: number
  is_furiten: boolean
  furiten_rate: number
  improves: ImproveEntry[]
  improve_way_count: number
  avg_improve_waits_count: number
  dama_point: number
  riichi_point: number
  mixed_round_point: number
  yaku_ids: number[]
}

export type DiscardCandidate = { discard: string; result: Hand13Result }

export type Hand14Result = {
  shanten: number
  maintain: DiscardCandidate[]
  backwards: DiscardCandidate[]
}

export type OpponentRisk = {
  seat: number
  tenpai_rate: number
  risk: number[]
  is_riichi: boolean
}

export type AnalysisResult = {
  seat: number
  turn: number
  shanten: number
  state: 'wait13' | 'discard14'
  hand13: Hand13Result | null
  hand14: Hand14Result | null
  opponents: OpponentRisk[]
  mixed_risk: number[]
  best_attack_discard: string | null
  best_defence_discard: string | null
}

export type DiscardEntry = { tile: string; tedashi: boolean; is_riichi: boolean }

export type MeldSnapshot = {
  kind: 'chi' | 'pon' | 'daiminkan' | 'ankan' | 'kakan'
  tiles: string[]
  from_who: number
  called_tile: string | null
}

export type PlayerSnapshot = {
  seat: number
  tehai: string[]
  melds: MeldSnapshot[]
  river: DiscardEntry[]
  score: number
  riichi_declared: boolean
  riichi_stage: boolean
  double_riichi: boolean
  riichi_declaration_index: number | null
  /** 3p only: north tiles set aside via kita / nukidora. Empty in 4p. */
  kita_tiles: string[]
}

export type GameStateSnapshot = {
  bakaze: 'E' | 'S' | 'W' | 'N'
  kyoku: number
  honba: number
  kyotaku: number
  oya: number
  current_player: number
  turn_count: number
  phase: 'wait_act' | 'wait_response'
  is_done: boolean
  /** 3 (sanma) or 4 (yonma). */
  num_players: number
  /** Length matches num_players. */
  players: PlayerSnapshot[]
  dora_markers: string[]
  our_seat: number | null
}

export type PlayerMahgenView = {
  seat: number
  hand: string
  melds: string[]
  river: string
}

export type MahgenView = {
  /** Length matches num_players. */
  players: PlayerMahgenView[]
  /** 3 (sanma) or 4 (yonma). */
  num_players: number
  dora_indicators: string
}

/** Mirrors `crate::schema::HoraScoreInfo`. Returned by `compute_bot_hora_score`. */
export type HoraScoreInfo = {
  points: number
  han: number
  fu: number
  yakuman: boolean
  /** mjai tile string of the winning tile. */
  win_tile: string
}

// ---------- Game History ----------
//
// Mirrors `crate::schema::history::*`. Strings carry RFC3339 timestamps;
// the frontend parses them with `new Date(...)` on demand.

export type Platform =
  | 'majsoul'
  | 'tenhou'
  | 'riichi_city'
  | 'mjai'
  | 'unknown'

export type KyokuMode = 'east_only' | 'east_south' | 'other'

/** Per-game stat counters from the recorded player's perspective. */
export type GameStats = {
  round: number
  oya: number

  fuuro: number
  fuuro_num: number
  fuuro_point: number
  fuuro_agari: number
  fuuro_agari_jun: number
  fuuro_agari_point: number
  fuuro_houjuu: number

  agari: number
  agari_as_oya: number
  agari_jun: number
  agari_point_oya: number
  agari_point_ko: number

  houjuu: number
  houjuu_jun: number
  houjuu_to_oya: number
  houjuu_point_to_oya: number
  houjuu_point_to_ko: number

  riichi: number
  riichi_as_oya: number
  riichi_jun: number
  riichi_agari: number
  riichi_agari_point: number
  riichi_agari_jun: number
  riichi_houjuu: number
  riichi_ryukyoku: number
  riichi_point: number
  chasing_riichi: number
  riichi_got_chased: number

  dama_agari: number
  dama_agari_jun: number
  dama_agari_point: number

  ryukyoku: number
  ryukyoku_point: number

  yakuman: number
  nagashi_mangan: number
}

export type GameRecord = {
  id: string
  /** RFC3339 timestamp. */
  started_at: string
  /** RFC3339 timestamp. */
  ended_at: string
  platform: Platform
  num_players: 3 | 4
  kyoku_mode: KyokuMode
  names: string[]
  our_seat: number | null
  final_scores: number[]
  final_ranks: number[]
  our_rank: number | null
  /** `final_score - starting_score` (4p:25000, 3p:35000). */
  our_delta: number | null
  stats: GameStats
  log_path: string
}

export type HistoryFilter = {
  platform?: Platform
  num_players?: 3 | 4
  kyoku_mode?: KyokuMode
  /** RFC3339 timestamp; inclusive. */
  started_after?: string
  /** RFC3339 timestamp; exclusive. */
  started_before?: string
}

export type HistoryEvent =
  | { kind: 'recorded'; record: GameRecord }
  | { kind: 'deleted'; id: string }

// ---------- Logs ----------
//
// Mirrors `crate::schema::ipc::{LogEntry, LogSessionInfo, ReadLogRequest,
// ReadLogResponse}`. The same shape is used both for entries read off
// disk (`read_log_session`) and for live-tailed entries delivered over a
// `tauri::ipc::Channel` (`subscribe_log_events`) — initial-load and live
// arrivals merge into the same UI list without translation.

export type LogLevel = 'TRACE' | 'DEBUG' | 'INFO' | 'WARN' | 'ERROR'

export type LogEntry = {
  ts_ms: number
  /** One of `LogLevel`, but kept open as `string` because backend may add levels. */
  level: string
  target: string
  file?: string
  line?: number
  message: string
  fields?: Record<string, unknown>
}

export type LogSessionInfo = {
  name: string
  path: string
  size_bytes: number
  mtime_ms: number
  is_active: boolean
}

export type ReadLogRequest = {
  session: string
  offset?: number
  limit?: number
  levels?: string[]
  /** Target prefixes; any-match (OR). */
  targets?: string[]
  /** Case-insensitive substring on `message`. */
  search?: string
}

export type ReadLogResponse = {
  entries: LogEntry[]
  has_more: boolean
  skipped_malformed: number
}

// ---------- Inspector ----------
//
// Mirrors `crate::schema::inspector::*` and `crate::schema::ipc::ReadInspector*`.
// Tagged on `kind` — switch on the discriminant to render kind-specific
// detail panels. Same shape arrives via `subscribe_inspector` (live tail)
// and `read_inspector` (past sessions), so renderers don't fork.

export type FrameDirection = 'up' | 'down'

export type FrameRaw =
  | { format: 'text'; data: string }
  | { format: 'binary'; data: string } // base64

export type ParsedFrame = {
  method: string
  args: unknown
}

export type BotReactionPayload = {
  bot: string
  actor_id: number
  trigger: MjaiEvent
  action: MjaiEvent
  meta?: Record<string, unknown>
  reaction_ms: number
}

export type InspectorEntry =
  | {
      kind: 'ws_frame'
      ts_ms: number
      direction: FrameDirection
      flow_id: string
      size: number
      raw: FrameRaw
      parsed?: ParsedFrame
      emitted: number
    }
  | {
      kind: 'mjai_event'
      ts_ms: number
      event: MjaiEvent
    }
  | {
      kind: 'bot_reaction'
      ts_ms: number
      // Backend serializes BotReaction with #[serde(flatten)], so its
      // fields land at the top level of the row alongside `kind` and
      // `ts_ms`.
      bot: string
      actor_id: number
      trigger: MjaiEvent
      action: MjaiEvent
      meta?: Record<string, unknown>
      reaction_ms: number
    }

export type InspectorKind = InspectorEntry['kind']

export type ReadInspectorRequest = {
  session: string
  offset?: number
  limit?: number
  kinds?: InspectorKind[]
  actor?: number
  search?: string
}

export type ReadInspectorResponse = {
  entries: InspectorEntry[]
  has_more: boolean
  skipped_malformed: number
}
