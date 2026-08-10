//! HTTP client for the remote inference API (`/v3/*`), as published by the
//! inference server's own API documentation.
//!
//! The service is **stateless**: every [`ApiClient::react`] call uploads the
//! current kyoku's mjai event stream (from the bot's seat perspective, ending
//! at the decision point) and gets back the move to play. This module is a thin
//! typed wrapper around those endpoints — the mjai stream shaping / censoring
//! lives in [`crate::bot::native`], the caller.
//!
//! Two consumers:
//! - the built-in bot ([`crate::bot::native::NativeBot`]) calls
//!   [`ApiClient::react`] at each decision point, while cloud inference is on;
//! - the IPC layer ([`crate::ipc::commands`]) calls [`redeem`],
//!   [`ApiClient::key_status`], [`ApiClient::models`] and [`health`] so the
//!   frontend can redeem codes and inspect a key.

use crate::config::NativeApiProvider;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Timeout for the management endpoints (key status, models, redeem, health).
/// These run from the UI, never on a game's critical path, so they can afford
/// to wait out a slow server.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

/// Timeout for the original [`ApiClient::react`] protocol on a live game's
/// critical path. A reach may need two calls, and the native bot shares this
/// single budget between them.
pub const REACT_TIMEOUT: Duration = Duration::from_millis(2_000);

/// Compute window granted to the FlyA runtime in every `/decision` request.
pub(crate) const FLYA_DEADLINE_MS: u64 = 8_000;

/// Client-side cap for FlyA decisions: the granted runtime deadline plus
/// network slack. This MUST exceed [`FLYA_DEADLINE_MS`].
pub(crate) const FLYA_REQUEST_TIMEOUT: Duration = Duration::from_millis(FLYA_DEADLINE_MS + 2_000);

/// Leave a small part of a caller-supplied turn budget for response transit and
/// parsing when telling the FlyA runtime how long it may compute.
const FLYA_NETWORK_SLACK_MS: u64 = 250;

/// Response from `POST /v3/react`.
#[derive(Debug, Clone, Deserialize)]
pub struct ReactResponse {
    /// The move to play, as a standard mjai event (`actor == player_id`).
    /// `None` when the seat has no legal action for the final event.
    #[serde(default)]
    pub reaction: Option<Value>,
    /// Up to `topk` coarse action labels ranked by probability. `candidates[0]`
    /// corresponds to `reaction`.
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    /// The model id that actually served the request.
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct FlyaDecisionResponse {
    protocol: String,
    model_id: String,
    attempt: FlyaAttempt,
}

/// Runtime attempt. Per openapi.json, `failure`/`timeout`/`abstain` attempts
/// omit the success-only fields and may carry an `error` object shaped
/// `{code, message}` (note: not the pre-runtime `{error, message}` shape).
#[derive(Debug, Clone, Deserialize)]
struct FlyaAttempt {
    status: String,
    #[serde(default)]
    selected_action_id: Option<u64>,
    #[serde(default)]
    action: Option<Value>,
    #[serde(default)]
    actions: Vec<FlyaActionCandidate>,
    #[serde(default)]
    error: Option<FlyaAttemptError>,
}

#[derive(Debug, Clone, Deserialize)]
struct FlyaAttemptError {
    code: String,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct FlyaActionCandidate {
    action_id: u64,
    action: Value,
    probability: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct FlyaModelsResponse {
    #[serde(default)]
    models: Vec<FlyaModelInfo>,
}

#[derive(Debug, Default, Deserialize)]
struct FlyaErrorResponse {
    #[serde(default)]
    error: String,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct FlyaModelInfo {
    model_id: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    rule_line: String,
    #[serde(default)]
    available: bool,
}

/// One entry of the policy's top-k distribution. `action` is a coarse label
/// (e.g. `dahai:W`, `reach`, `pon`) — the exact tiles are in `reaction`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub action: String,
    #[serde(default)]
    pub prob: f64,
}

/// Response from `GET /v3/key` — the key's plan, expiry and live limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyStatus {
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default)]
    pub usage_today: u64,
    #[serde(default)]
    pub rpd: u64,
    /// Requests/minute. The server reports this as a float (e.g. `10.0`), so it
    /// is typed `f64` — deserializing it into an integer would fail the parse.
    #[serde(default)]
    pub rpm: f64,
    #[serde(default)]
    pub topk: u32,
}

/// One model the key's plan may use (`GET /v3/models`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default)]
    pub game: String,
    #[serde(default)]
    pub desc: String,
}

/// Response from `POST /v3/redeem`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedeemResponse {
    /// The raw 32-char key — present **only** when a new key is minted
    /// (`extended == false`). Never re-shown on a renewal.
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub key_last4: String,
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub expires_at: String,
    /// `true` when time was stacked onto an existing key (no new key issued).
    #[serde(default)]
    pub extended: bool,
}

/// Response from `GET /healthz` — liveness + per-model queue depth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub queue_depth: BTreeMap<String, i64>,
}

#[derive(Serialize)]
struct ReactRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    player_id: u8,
    events: Vec<Value>,
}

#[derive(Serialize)]
struct RedeemRequest<'a> {
    code: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    renew_key: Option<&'a str>,
}

/// Authenticated client bound to one server + key.
///
/// Each `ApiClient` builds its own `reqwest::Client`, and with it a fresh
/// connection pool — so **hold one and reuse it** rather than constructing one
/// per request, or every call pays a new TCP + TLS handshake. The built-in bot
/// keeps one alive across a game and only rebuilds it when the server URL or
/// key changes.
pub struct ApiClient {
    provider: NativeApiProvider,
    base: String,
    key: String,
    http: reqwest::Client,
}

impl ApiClient {
    /// Build a client for `base_url` authenticating with `key`, optionally
    /// routing through `proxy` (see [`http_client`]; empty ⇒ direct). A
    /// trailing slash on the URL is tolerated.
    pub fn new(
        provider: NativeApiProvider,
        base_url: &str,
        key: &str,
        proxy: &str,
    ) -> Result<Self> {
        Ok(Self {
            provider,
            base: normalize_base(base_url),
            key: key.trim().to_string(),
            http: http_client(REQUEST_TIMEOUT, proxy)?,
        })
    }

    /// True when the configured endpoint is FlyAPI's `/beta/v1` service.
    /// Both the root host and the full `/beta/v1` URL are accepted.
    pub fn is_flya(&self) -> bool {
        self.provider == NativeApiProvider::Flya
    }

    /// `POST /v3/react` — the move for the final event of `events`. `model`
    /// `None`/empty lets the server pick its game default.
    ///
    /// Bounded by [`REACT_TIMEOUT`] rather than the client-wide
    /// [`REQUEST_TIMEOUT`]: this one blocks the bot's turn.
    pub async fn react(
        &self,
        model: Option<&str>,
        player_id: u8,
        events: Vec<Value>,
    ) -> Result<ReactResponse> {
        let timeout = if self.is_flya() {
            FLYA_REQUEST_TIMEOUT
        } else {
            REACT_TIMEOUT
        };
        self.react_with_timeout(model, player_id, events, timeout)
            .await
    }

    /// The same request as [`ApiClient::react`], bounded by a caller-supplied
    /// timeout. The native bot uses this for the second half of reach so both
    /// calls share one turn-level deadline.
    pub(crate) async fn react_with_timeout(
        &self,
        model: Option<&str>,
        player_id: u8,
        events: Vec<Value>,
        timeout: Duration,
    ) -> Result<ReactResponse> {
        if timeout.is_zero() {
            bail!("POST /v3/react: request budget exhausted");
        }
        if self.is_flya() {
            let deadline = Instant::now() + timeout;
            return tokio::time::timeout(
                timeout,
                self.react_flya(model, player_id, events, deadline),
            )
            .await
            .context("POST /beta/v1/decision: request budget exhausted")?;
        }
        let url = format!("{}/v3/react", self.base);
        let body = ReactRequest {
            model: model.filter(|m| !m.is_empty()),
            player_id,
            events,
        };
        let resp = self
            .http
            .post(&url)
            .timeout(timeout)
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await
            .context("POST /v3/react")?;
        let resp = check(resp, "react").await?;
        resp.json::<ReactResponse>()
            .await
            .context("parse /v3/react response")
    }

    async fn react_flya(
        &self,
        model: Option<&str>,
        player_id: u8,
        events: Vec<Value>,
        deadline: Instant,
    ) -> Result<ReactResponse> {
        let rule_line = flya_rule_line(&events)?;
        let state_digest = flya_state_digest(&events)?;
        let request_id = flya_request_id(rule_line, player_id, &state_digest, model);
        let state = serde_json::json!({
            "schema": "flya-mahjong-events-v2",
            "rule_line": rule_line,
            "viewer_seat": player_id,
            "source": { "kind": "observed" },
            "from_seq": 0,
            "to_seq": events.len(),
            "events": events,
            "state_digest": state_digest,
        });
        let mut body = serde_json::Map::new();
        body.insert("request_id".into(), Value::String(request_id));
        if let Some(model) = model.filter(|value| !value.trim().is_empty()) {
            body.insert("model_id".into(), Value::String(model.to_string()));
        }
        body.insert("state".into(), state);
        body.insert(
            "deadline_ms".into(),
            Value::from(flya_runtime_deadline_ms(deadline)?),
        );

        let url = format!("{}/decision", flya_base(&self.base));
        let request_body = Value::Object(body.clone());
        let mut resp = self
            .http
            .post(&url)
            .timeout(remaining_budget(deadline, "FlyA decision")?)
            .bearer_auth(&self.key)
            .json(&request_body)
            .send()
            .await
            .context("POST /beta/v1/decision")?;
        if resp.status() == reqwest::StatusCode::FORBIDDEN {
            let status = resp.status();
            let raw = resp.text().await.unwrap_or_default();
            let error = serde_json::from_str::<FlyaErrorResponse>(&raw).unwrap_or_default();
            if error.error == "test_api_key_frozen" {
                self.activate_flya(deadline).await?;
                resp = self
                    .http
                    .post(&url)
                    .timeout(remaining_budget(deadline, "FlyA decision retry")?)
                    .bearer_auth(&self.key)
                    .json(&request_body)
                    .send()
                    .await
                    .context("POST /beta/v1/decision retry")?;
            } else {
                let detail = match (error.error.is_empty(), error.message.is_empty()) {
                    (false, false) => format!("{}: {}", error.error, error.message),
                    (false, true) => error.error,
                    (true, false) => error.message,
                    (true, true) if raw.trim().is_empty() => "empty response".to_string(),
                    (true, true) => raw,
                };
                bail!("FlyA decision failed: HTTP {status}: {detail}");
            }
        }
        let resp = check(resp, "FlyA decision").await?;
        let parsed = resp
            .json::<FlyaDecisionResponse>()
            .await
            .context("parse FlyA decision response")?;
        let events = body["state"]["events"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        flya_response_to_react(parsed, model, player_id, events)
    }

    async fn activate_flya(&self, deadline: Instant) -> Result<()> {
        let url = format!("{}/quota", flya_base(&self.base));
        let resp = self
            .http
            .get(&url)
            .timeout(remaining_budget(deadline, "FlyA quota activation")?)
            .bearer_auth(&self.key)
            .send()
            .await
            .context("GET /beta/v1/quota activation")?;
        let resp = check(resp, "FlyA quota activation").await?;
        let value = resp
            .json::<Value>()
            .await
            .context("parse FlyA quota activation response")?;
        if !matches!(
            value.get("status").and_then(Value::as_str),
            Some("active" | "grace")
        ) {
            bail!("FlyA quota activation returned an invalid status");
        }
        Ok(())
    }

    /// `GET /v3/key` — the key's plan, expiry and live rate limits.
    pub async fn key_status(&self) -> Result<KeyStatus> {
        if self.is_flya() {
            let url = format!("{}/quota", flya_base(&self.base));
            let resp = self
                .http
                .get(&url)
                .bearer_auth(&self.key)
                .send()
                .await
                .context("GET /beta/v1/quota")?;
            let resp = check(resp, "FlyA quota").await?;
            let value = resp
                .json::<Value>()
                .await
                .context("parse FlyA quota response")?;
            return Ok(flya_quota_to_key_status(&value));
        }
        let url = format!("{}/v3/key", self.base);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.key)
            .send()
            .await
            .context("GET /v3/key")?;
        let resp = check(resp, "key status").await?;
        resp.json::<KeyStatus>()
            .await
            .context("parse /v3/key response")
    }

    /// `GET /v3/models` — the models this key's plan may use.
    pub async fn models(&self) -> Result<Vec<ModelInfo>> {
        if self.is_flya() {
            let url = format!("{}/models", flya_base(&self.base));
            let resp = self
                .http
                .get(&url)
                .bearer_auth(&self.key)
                .send()
                .await
                .context("GET /beta/v1/models")?;
            let resp = check(resp, "FlyA models").await?;
            let payload = resp
                .json::<FlyaModelsResponse>()
                .await
                .context("parse FlyA models response")?;
            return Ok(payload
                .models
                .into_iter()
                .filter(|model| model.available)
                .map(|model| ModelInfo {
                    id: model.model_id,
                    game: if model.rule_line == "riichi3p" {
                        "3p".into()
                    } else {
                        "4p".into()
                    },
                    desc: model.display_name,
                })
                .collect());
        }
        let url = format!("{}/v3/models", self.base);
        let resp = self
            .http
            .get(&url)
            .bearer_auth(&self.key)
            .send()
            .await
            .context("GET /v3/models")?;
        let resp = check(resp, "models").await?;
        #[derive(Deserialize)]
        struct Wrap {
            #[serde(default)]
            models: Vec<ModelInfo>,
        }
        Ok(resp
            .json::<Wrap>()
            .await
            .context("parse /v3/models response")?
            .models)
    }
}

/// `POST /v3/redeem` (no auth). By default mints a **new** key; pass
/// `renew_key` to stack time onto a key you already hold. `email` links minted
/// keys to an account (ignored when `renew_key` is set).
pub async fn redeem(
    base_url: &str,
    proxy: &str,
    code: &str,
    email: Option<&str>,
    renew_key: Option<&str>,
) -> Result<RedeemResponse> {
    let base = normalize_base(base_url);
    let http = http_client(REQUEST_TIMEOUT, proxy)?;
    let url = format!("{base}/v3/redeem");
    let body = RedeemRequest {
        code: code.trim(),
        email: email.map(str::trim).filter(|s| !s.is_empty()),
        renew_key: renew_key.map(str::trim).filter(|s| !s.is_empty()),
    };
    let resp = http
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("POST /v3/redeem")?;
    let resp = check(resp, "redeem").await?;
    resp.json::<RedeemResponse>()
        .await
        .context("parse /v3/redeem response")
}

/// `GET /healthz` (no auth) — liveness + per-model queue depth.
pub async fn health(base_url: &str, proxy: &str) -> Result<Health> {
    let base = normalize_base(base_url);
    let http = http_client(REQUEST_TIMEOUT, proxy)?;
    let url = format!("{base}/healthz");
    let resp = http.get(&url).send().await.context("GET /healthz")?;
    let resp = check(resp, "health").await?;
    resp.json::<Health>()
        .await
        .context("parse /healthz response")
}

/// Build the HTTP client all inference-server traffic goes through, honoring
/// the user's proxy setting (`bot.api.proxy`). `proxy` accepts
/// `http://`, `https://`, `socks5://` or `socks5h://` URLs (`socks5h` resolves
/// DNS on the proxy — what you want when the server's name doesn't resolve
/// locally); empty/whitespace ⇒ direct connection. Shared with
/// [`crate::bot::purchase`], so the whole `/v3` + `/paypal` surface follows
/// one setting.
pub(crate) fn http_client(timeout: Duration, proxy: &str) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().timeout(timeout);
    let proxy = proxy.trim();
    if !proxy.is_empty() {
        // Scheme match is case-insensitive (RFC 3986; the url crate lowercases
        // on parse anyway) — `HTTP://…` pasted from a provider dashboard works.
        const SCHEMES: [&str; 4] = ["http://", "https://", "socks5://", "socks5h://"];
        let scheme_ok = SCHEMES.iter().any(|s| {
            proxy
                .get(..s.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(s))
        });
        // Never echo the proxy string into errors: proxy URLs routinely carry
        // credentials (`socks5://user:pass@host`), and these messages end up
        // in toasts AND in the persisted session log (which users attach to
        // bug reports). The user just typed the value — no diagnostic value
        // in repeating it.
        if !scheme_ok {
            bail!("unsupported proxy scheme: use http://, https://, socks5:// or socks5h://");
        }
        let p = reqwest::Proxy::all(proxy).context("invalid proxy URL")?;
        builder = builder.proxy(p);
    }
    builder.build().context("build inference-API http client")
}

pub(crate) fn normalize_base(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn flya_base(base: &str) -> String {
    let base = base.trim_end_matches('/');
    if base.to_ascii_lowercase().ends_with("/beta/v1") {
        base.to_string()
    } else {
        format!("{base}/beta/v1")
    }
}

fn remaining_budget(deadline: Instant, operation: &str) -> Result<Duration> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        bail!("{operation}: request budget exhausted");
    }
    Ok(remaining)
}

fn flya_runtime_deadline_ms(deadline: Instant) -> Result<u64> {
    let remaining = remaining_budget(deadline, "FlyA decision")?;
    let remaining_ms = u64::try_from(remaining.as_millis()).unwrap_or(u64::MAX);
    Ok(remaining_ms
        .saturating_sub(FLYA_NETWORK_SLACK_MS)
        .clamp(1, FLYA_DEADLINE_MS))
}

fn flya_rule_line(events: &[Value]) -> Result<&'static str> {
    let seats = events.iter().find_map(|event| {
        let kind = event.get("type").and_then(Value::as_str)?;
        match kind {
            "start_kyoku" => event.get("scores").and_then(Value::as_array).map(Vec::len),
            "start_game" => event.get("names").and_then(Value::as_array).map(Vec::len),
            _ => None,
        }
    });
    match seats {
        Some(3) => Ok("riichi3p"),
        Some(4) => Ok("riichi4p"),
        Some(other) => bail!("FlyA state has unsupported seat count {other}"),
        None => bail!("FlyA state is missing start_game/start_kyoku seat data"),
    }
}

fn flya_state_digest(events: &[Value]) -> Result<String> {
    let mut canonical = Vec::with_capacity(events.len());
    for event in events {
        let object = event
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("FlyA event is not a JSON object"))?;
        let event_type = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("FlyA event has no string type"))?;
        let mut ordered = serde_json::Map::new();
        ordered.insert("type".into(), Value::String(event_type.to_string()));
        let mut keys: Vec<&String> = object.keys().filter(|key| key.as_str() != "type").collect();
        keys.sort();
        for key in keys {
            ordered.insert(key.clone(), flya_sorted_json(&object[key]));
        }
        canonical.push(Value::Object(ordered));
    }
    let bytes = serde_json::to_vec(&canonical).context("serialize FlyA state digest")?;
    let mut digest = 0xcbf29ce484222325u64;
    for byte in bytes {
        digest ^= u64::from(byte);
        digest = digest.wrapping_mul(0x100000001b3);
    }
    Ok(format!("fnv1a64:{digest:016x}"))
}

fn flya_sorted_json(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort();
            for key in keys {
                sorted.insert(key.clone(), flya_sorted_json(&object[key]));
            }
            Value::Object(sorted)
        }
        Value::Array(values) => Value::Array(values.iter().map(flya_sorted_json).collect()),
        other => other.clone(),
    }
}

fn flya_request_id(
    rule_line: &str,
    player_id: u8,
    state_digest: &str,
    model: Option<&str>,
) -> String {
    let model = model
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("default");
    let model: String = model
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || "._-:".contains(ch) {
                ch
            } else {
                '-'
            }
        })
        .take(72)
        .collect();
    let model = if model.is_empty() {
        "default"
    } else {
        model.as_str()
    };
    format!(
        "akagi-{rule_line}-{player_id}-{}-{model}",
        state_digest.trim_start_matches("fnv1a64:")
    )
}

fn flya_response_to_react(
    response: FlyaDecisionResponse,
    requested_model: Option<&str>,
    player_id: u8,
    events: &[Value],
) -> Result<ReactResponse> {
    if response.protocol != "flya-test-api-v1" {
        bail!("FlyA response has an unsupported protocol");
    }
    if response.model_id.trim().is_empty() {
        bail!("FlyA response has no model_id");
    }
    if requested_model
        .filter(|value| !value.trim().is_empty())
        .is_some_and(|value| value != response.model_id)
    {
        bail!("FlyA response model_id does not match the requested model");
    }
    if response.attempt.status != "success" {
        let detail = response
            .attempt
            .error
            .map(|error| format!(" ({}: {})", error.code, error.message))
            .unwrap_or_default();
        bail!(
            "FlyA decision attempt status is {}{detail}",
            response.attempt.status
        );
    }
    if response.attempt.actions.is_empty() {
        bail!("FlyA response has no candidate actions");
    }
    let selected_action_id = response
        .attempt
        .selected_action_id
        .ok_or_else(|| anyhow::anyhow!("FlyA success attempt has no selected_action_id"))?;
    let selected_action = response
        .attempt
        .action
        .clone()
        .ok_or_else(|| anyhow::anyhow!("FlyA success attempt has no selected action"))?;

    let mut seen_ids = std::collections::BTreeSet::new();
    let mut candidates = Vec::with_capacity(response.attempt.actions.len());
    let mut probability_sum = 0.0f64;
    for candidate in &response.attempt.actions {
        if !seen_ids.insert(candidate.action_id)
            || !candidate.probability.is_finite()
            || !(0.0 < candidate.probability && candidate.probability <= 1.0)
        {
            bail!("FlyA response has invalid candidate actions");
        }
        let label = flya_action_label(&candidate.action)?;
        probability_sum += candidate.probability;
        candidates.push((
            candidate.action_id,
            candidate.action.clone(),
            label,
            candidate.probability,
        ));
    }
    if (probability_sum - 1.0).abs() > 1e-6 {
        bail!("FlyA response candidate probabilities do not sum to one");
    }
    let selected_index = candidates
        .iter()
        .position(|(id, _, _, _)| *id == selected_action_id)
        .ok_or_else(|| anyhow::anyhow!("FlyA response selected_action_id is missing"))?;
    if selected_action != candidates[selected_index].1 {
        bail!("FlyA response selected action does not match selected_action_id");
    }

    let reaction = flya_action_to_mjai(&candidates[selected_index].1, player_id, events)?;
    let selected_id = candidates[selected_index].0;
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by(|left, right| {
        if *left == selected_index {
            std::cmp::Ordering::Less
        } else if *right == selected_index {
            std::cmp::Ordering::Greater
        } else {
            candidates[*right]
                .3
                .partial_cmp(&candidates[*left].3)
                .unwrap_or(std::cmp::Ordering::Equal)
        }
    });
    let visible = order
        .into_iter()
        .take(3)
        .map(|index| Candidate {
            action: candidates[index].2.clone(),
            prob: candidates[index].3,
        })
        .collect();
    debug_assert_eq!(selected_id, selected_action_id);
    Ok(ReactResponse {
        reaction: Some(reaction),
        candidates: visible,
        model: Some(response.model_id),
    })
}

fn flya_action_label(action: &Value) -> Result<String> {
    let action_type = action
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("FlyA action has no string type"))?;
    let label = match action_type {
        "dahai" => {
            flya_string(action, "pai")?;
            if action.get("tsumogiri").and_then(Value::as_bool).is_none() {
                bail!("FlyA dahai action has no boolean tsumogiri");
            }
            format!("dahai:{}", flya_string(action, "pai")?)
        }
        "dealer_opening_dahai" => format!("dahai:{}", flya_string(action, "pai")?),
        "riichi_dahai" | "dealer_opening_riichi_dahai" => {
            flya_string(action, "pai")?;
            "reach".into()
        }
        "chi" | "pon" | "daiminkan" | "ankan" | "kakan" => {
            let consumed = flya_tiles(action, "consumed")?;
            let expected = if action_type == "ankan" {
                4
            } else if action_type == "kakan" || action_type == "daiminkan" {
                3
            } else {
                2
            };
            if consumed.len() != expected {
                bail!("FlyA {action_type} action has the wrong consumed tile count");
            }
            if action_type != "ankan" {
                flya_string(action, "pai")?;
            }
            match action_type {
                "chi" => "chi_mid".into(),
                "pon" => "pon".into(),
                _ => "kan".into(),
            }
        }
        "kita" => "nukidora".into(),
        "tsumo" | "ron" => {
            if action_type == "ron" {
                action
                    .get("target")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| anyhow::anyhow!("FlyA ron action has no target"))?;
            }
            "hora".into()
        }
        "kyushukyuhai" => "ryukyoku".into(),
        "pass_all" => "none".into(),
        other => bail!("FlyA response contains unsupported action type {other}"),
    };
    Ok(label)
}

fn flya_action_to_mjai(action: &Value, player_id: u8, events: &[Value]) -> Result<Value> {
    let action_type = action
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("FlyA action has no string type"))?;
    let target = || {
        events.iter().rev().find_map(|event| {
            let actor = event.get("actor").and_then(Value::as_u64)? as u8;
            let is_discard = matches!(
                event.get("type").and_then(Value::as_str),
                Some("dahai") | Some("dealer_opening_dahai")
            );
            (is_discard && actor != player_id).then_some(actor)
        })
    };
    let result = match action_type {
        "dahai" => serde_json::json!({
            "type": "dahai",
            "actor": player_id,
            "pai": flya_string(action, "pai")?,
            "tsumogiri": action.get("tsumogiri").and_then(Value::as_bool).ok_or_else(|| anyhow::anyhow!("FlyA dahai action has no tsumogiri"))?,
        }),
        "dealer_opening_dahai" => serde_json::json!({
            "type": "dahai",
            "actor": player_id,
            "pai": flya_string(action, "pai")?,
            "tsumogiri": false,
        }),
        "riichi_dahai" | "dealer_opening_riichi_dahai" => serde_json::json!({
            "type": "reach",
            "actor": player_id,
            "pai": flya_string(action, "pai")?,
        }),
        "chi" | "pon" | "daiminkan" => serde_json::json!({
            "type": action_type,
            "actor": player_id,
            "target": target().ok_or_else(|| anyhow::anyhow!("FlyA call action has no preceding discard"))?,
            "pai": flya_string(action, "pai")?,
            "consumed": flya_tiles(action, "consumed")?,
        }),
        "ankan" => serde_json::json!({
            "type": "ankan",
            "actor": player_id,
            "consumed": flya_tiles(action, "consumed")?,
        }),
        "kakan" => serde_json::json!({
            "type": "kakan",
            "actor": player_id,
            "pai": flya_string(action, "pai")?,
            "consumed": flya_tiles(action, "consumed")?,
        }),
        "kita" => serde_json::json!({ "type": "kita", "actor": player_id, "pai": "N" }),
        "tsumo" => serde_json::json!({ "type": "hora", "actor": player_id, "target": player_id }),
        "ron" => serde_json::json!({
            "type": "hora",
            "actor": player_id,
            "target": action.get("target").and_then(Value::as_u64).ok_or_else(|| anyhow::anyhow!("FlyA ron action has no target"))?,
        }),
        "kyushukyuhai" => serde_json::json!({ "type": "ryukyoku" }),
        "pass_all" => serde_json::json!({ "type": "none" }),
        other => bail!("FlyA response contains unsupported action type {other}"),
    };
    Ok(result)
}

fn flya_string(action: &Value, name: &str) -> Result<String> {
    action
        .get(name)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow::anyhow!("FlyA action field {name} is not a string"))
}

fn flya_tiles(action: &Value, name: &str) -> Result<Vec<String>> {
    action
        .get(name)
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("FlyA action field {name} is not an array"))?
        .iter()
        .map(|tile| {
            tile.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                anyhow::anyhow!("FlyA action field {name} contains a non-string tile")
            })
        })
        .collect()
}

fn flya_quota_to_key_status(value: &Value) -> KeyStatus {
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let kind = value
        .get("key_kind")
        .and_then(Value::as_str)
        .unwrap_or("test");
    let total = value
        .get("quota_total")
        .or_else(|| value.get("total"))
        .or_else(|| {
            value
                .get("five_hour")
                .and_then(|window| window.get("limit"))
        });
    let used = value
        .get("quota_used")
        .or_else(|| value.get("used"))
        .or_else(|| value.get("five_hour").and_then(|window| window.get("used")));
    KeyStatus {
        plan: format!("FlyA {kind} ({status})"),
        expires_at: value
            .get("expires_at")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        usage_today: flya_decimal_u64(used),
        rpd: flya_decimal_u64(total),
        rpm: 0.0,
        topk: 3,
    }
}

fn flya_decimal_u64(value: Option<&Value>) -> u64 {
    if let Some(number) = value.and_then(Value::as_u64) {
        return number;
    }
    value
        .and_then(Value::as_str)
        .and_then(|raw| raw.split('.').next())
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(0)
}

/// Turn a non-2xx response into a descriptive error, surfacing the server's
/// generic `{"error": "..."}` message and any `Retry-After` hint. Success
/// passes the response through untouched for the caller to deserialize.
/// Shared with the purchase client ([`crate::bot::purchase`]).
pub(crate) async fn check(resp: reqwest::Response, what: &str) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let retry_after = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let raw = resp.text().await.unwrap_or_default();
    // Pre-runtime errors are shaped {"error": code, "message": detail};
    // surface both — the detail line is often the actionable part (e.g.
    // which event index failed to replay, or why no decision is due).
    let msg = serde_json::from_str::<Value>(&raw)
        .ok()
        .and_then(|v| {
            let code = v.get("error").and_then(Value::as_str);
            let detail = v.get("message").and_then(Value::as_str);
            match (code, detail) {
                (Some(code), Some(detail)) => Some(format!("{code} — {detail}")),
                (Some(code), None) => Some(code.to_string()),
                _ => None,
            }
        })
        .unwrap_or_else(|| raw.chars().take(200).collect());
    let code = status.as_u16();
    match retry_after {
        Some(ra) => bail!("{what} failed: HTTP {code} — {msg} (retry after {ra}s)"),
        None => bail!("{what} failed: HTTP {code} — {msg}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_client_accepts_supported_proxy_schemes_and_empty() {
        for p in [
            "",
            "  ",
            "http://127.0.0.1:7890",
            "https://p:8443",
            "socks5://127.0.0.1:1080",
            "socks5h://127.0.0.1:1080",
            // Schemes are case-insensitive (RFC 3986).
            "HTTP://127.0.0.1:7890",
            "SOCKS5h://127.0.0.1:1080",
        ] {
            assert!(
                http_client(REQUEST_TIMEOUT, p).is_ok(),
                "proxy {p:?} should build"
            );
        }
    }

    #[test]
    fn http_client_rejects_unknown_proxy_scheme() {
        for p in [
            "ftp://x",
            "socks4://127.0.0.1:1080",
            "127.0.0.1:7890",
            "socks5:/bad",
        ] {
            let err = http_client(REQUEST_TIMEOUT, p).unwrap_err();
            assert!(
                format!("{err:#}").contains("unsupported proxy scheme"),
                "proxy {p:?} should be rejected with a clear message, got: {err:#}"
            );
        }
    }

    #[test]
    fn proxy_errors_never_echo_the_credentials() {
        // Proxy URLs often carry `user:pass@host`; the error text lands in
        // toasts and persisted logs, so it must not contain the secret.
        let err = http_client(REQUEST_TIMEOUT, "socks4://user:sekret@10.0.0.1:1080").unwrap_err();
        let text = format!("{err:#}");
        assert!(
            !text.contains("sekret") && !text.contains("10.0.0.1"),
            "error text leaked the proxy string: {text}"
        );
    }

    #[test]
    fn normalize_trims_trailing_slash_and_space() {
        assert_eq!(normalize_base("http://host:8080/"), "http://host:8080");
        assert_eq!(normalize_base("  https://host  "), "https://host");
        assert_eq!(normalize_base("http://host:8080"), "http://host:8080");
    }

    #[test]
    fn react_request_omits_empty_model() {
        let body = ReactRequest {
            model: None,
            player_id: 0,
            events: vec![],
        };
        let v = serde_json::to_value(&body).unwrap();
        assert!(v.get("model").is_none(), "model must be omitted when None");
        assert_eq!(v["player_id"], 0);
    }

    #[test]
    fn react_request_keeps_model_when_set() {
        let body = ReactRequest {
            model: Some("4p-ot2"),
            player_id: 2,
            events: vec![],
        };
        let v = serde_json::to_value(&body).unwrap();
        assert_eq!(v["model"], "4p-ot2");
        assert_eq!(v["player_id"], 2);
    }

    #[test]
    fn redeem_request_omits_blank_optionals() {
        let body = RedeemRequest {
            code: "abc",
            email: Some("  "),
            renew_key: None,
        };
        // Callers pass pre-filtered options; this mirrors the API's default
        // "mint a new key anonymously" path — code only.
        let filtered = RedeemRequest {
            code: body.code,
            email: body.email.map(str::trim).filter(|s| !s.is_empty()),
            renew_key: body.renew_key,
        };
        let v = serde_json::to_value(&filtered).unwrap();
        assert_eq!(v["code"], "abc");
        assert!(v.get("email").is_none());
        assert!(v.get("renew_key").is_none());
    }

    #[test]
    fn react_response_defaults_missing_fields() {
        let r: ReactResponse = serde_json::from_str("{}").unwrap();
        assert!(r.reaction.is_none());
        assert!(r.candidates.is_empty());
        assert!(r.model.is_none());
    }

    /// The live server returns `rpm` as a float (`10.0`); parsing it into an
    /// integer would fail, so `KeyStatus.rpm` is `f64`. Lock that in.
    #[test]
    fn key_status_parses_float_rpm() {
        let raw = r#"{"plan":"basic","expires_at":"2026-08-04 19:15:42","usage_today":3,"rpd":6000,"rpm":10.0,"topk":3}"#;
        let k: KeyStatus = serde_json::from_str(raw).unwrap();
        assert_eq!(k.plan, "basic");
        assert_eq!(k.usage_today, 3);
        assert_eq!(k.rpd, 6000);
        assert!((k.rpm - 10.0).abs() < 1e-9);
        assert_eq!(k.topk, 3);
    }

    #[test]
    fn models_wrapper_parses() {
        let raw = r#"{"models":[{"id":"4p-ot2","game":"4p","desc":"4p Mortal v4 (ot2), 192x40"},{"id":"3p-ot2","game":"3p","desc":"3p Mortal v4"}]}"#;
        #[derive(Deserialize)]
        struct Wrap {
            #[serde(default)]
            models: Vec<ModelInfo>,
        }
        let w: Wrap = serde_json::from_str(raw).unwrap();
        assert_eq!(w.models.len(), 2);
        assert_eq!(w.models[0].id, "4p-ot2");
        assert_eq!(w.models[1].game, "3p");
    }

    // ---- end-to-end against a local mock server ----

    use crate::bot::test_http::{mock_http, mock_http_with_delays};

    /// Value of `name` in a raw HTTP request head, case-insensitively.
    fn header<'a>(req: &'a str, name: &str) -> Option<&'a str> {
        req.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
        })
    }

    /// The bearer key must travel in the `Authorization` header — never in the
    /// URL or query string, where proxies and server access logs would capture it.
    #[tokio::test]
    async fn react_authenticates_with_a_bearer_header() {
        let (base, served) = mock_http(vec![(
            "200 OK",
            r#"{"reaction":{"type":"none"},"candidates":[{"action":"none","prob":1.0}],"model":"4p-x"}"#
                .into(),
        )]);
        let client = ApiClient::new(NativeApiProvider::Original, &base, "SECRETKEY", "").unwrap();
        let resp = client
            .react(
                Some("4p-x"),
                2,
                vec![serde_json::json!({"type": "start_game"})],
            )
            .await
            .unwrap();
        assert_eq!(resp.model.as_deref(), Some("4p-x"));

        let reqs = served.join().unwrap();
        let r = &reqs[0];
        assert!(r.starts_with("POST /v3/react "), "wrong request line: {r}");
        assert_eq!(header(r, "authorization"), Some("Bearer SECRETKEY"));
        assert!(
            !r.lines().next().unwrap().contains("SECRETKEY"),
            "key must not appear in the request line: {r}"
        );
        assert!(r.contains(r#""player_id":2"#), "{r}");
        assert!(r.contains(r#""model":"4p-x""#), "{r}");
    }

    #[test]
    fn flya_client_timeout_outlives_the_runtime_deadline() {
        assert!(FLYA_REQUEST_TIMEOUT > Duration::from_millis(FLYA_DEADLINE_MS));
        assert!(FLYA_REQUEST_TIMEOUT > REACT_TIMEOUT);
    }

    #[test]
    fn provider_routing_is_explicit_and_never_inferred_from_the_url() {
        let original = ApiClient::new(
            NativeApiProvider::Original,
            "https://api.nashout.com",
            "ORIGINAL_KEY",
            "",
        )
        .unwrap();
        let flya = ApiClient::new(
            NativeApiProvider::Flya,
            "https://flya.example.test",
            "FLYA_KEY",
            "",
        )
        .unwrap();
        let spoofed = ApiClient::new(
            NativeApiProvider::Original,
            "https://api.nashout.com.attacker.invalid",
            "ORIGINAL_KEY",
            "",
        )
        .unwrap();
        assert!(!original.is_flya());
        assert!(flya.is_flya());
        assert!(!spoofed.is_flya());
    }

    #[tokio::test]
    async fn caller_timeout_bounds_original_and_flya_requests() {
        for flya in [false, true] {
            let (base, _served) =
                mock_http_with_delays(vec![("200 OK", "{}".into(), Duration::from_millis(400))]);
            let base = if flya {
                format!("{base}/beta/v1")
            } else {
                base
            };
            let provider = if flya {
                NativeApiProvider::Flya
            } else {
                NativeApiProvider::Original
            };
            let client = ApiClient::new(provider, &base, "SECRETKEY", "").unwrap();
            let events = vec![serde_json::json!({
                "type": "start_game",
                "names": ["", "", "", ""],
                "seed": null,
            })];
            let started = std::time::Instant::now();
            let err = client
                .react_with_timeout(None, 0, events, Duration::from_millis(50))
                .await
                .unwrap_err();
            assert!(started.elapsed() < Duration::from_millis(300));
            assert!(!format!("{err:#}").is_empty());
        }
    }

    #[tokio::test]
    async fn caller_timeout_bounds_frozen_key_activation_and_retry_as_one_budget() {
        let (base, _served) = mock_http_with_delays(vec![
            (
                "403 Forbidden",
                r#"{"error":"test_api_key_frozen"}"#.into(),
                Duration::from_millis(20),
            ),
            (
                "200 OK",
                r#"{"status":"active"}"#.into(),
                Duration::from_millis(400),
            ),
            ("200 OK", "{}".into(), Duration::ZERO),
        ]);
        let client = ApiClient::new(NativeApiProvider::Flya, &base, "SECRETKEY", "").unwrap();
        let events = vec![serde_json::json!({
            "type": "start_game",
            "names": ["", "", "", ""],
            "seed": null,
        })];
        let started = Instant::now();
        let err = client
            .react_with_timeout(None, 0, events, Duration::from_millis(80))
            .await
            .unwrap_err();
        assert!(started.elapsed() < Duration::from_millis(250));
        assert!(!format!("{err:#}").is_empty());
    }

    #[tokio::test]
    async fn flya_models_omit_unavailable_entries() {
        let (base, served) = mock_http(vec![(
            "200 OK",
            r#"{"models":[{"model_id":"ready-4p","display_name":"Ready","rule_line":"riichi4p","available":true},{"model_id":"blocked-3p","display_name":"Blocked","rule_line":"riichi3p","available":false,"unavailable_reason":"quota"}]}"#.into(),
        )]);
        let client = ApiClient::new(
            NativeApiProvider::Flya,
            &format!("{base}/beta/v1"),
            "SECRETKEY",
            "",
        )
        .unwrap();
        let models = client.models().await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "ready-4p");
        assert_eq!(served.join().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn key_status_and_models_authenticate_with_a_bearer_header() {
        let (base, served) = mock_http(vec![
            (
                "200 OK",
                r#"{"plan":"basic","expires_at":"2026-08-04","usage_today":1,"rpd":6000,"rpm":10.0,"topk":3}"#.into(),
            ),
            (
                "200 OK",
                r#"{"models":[{"id":"4p-x","game":"4p","desc":"d"}]}"#.into(),
            ),
        ]);
        let client = ApiClient::new(NativeApiProvider::Original, &base, "SECRETKEY", "").unwrap();
        assert_eq!(client.key_status().await.unwrap().plan, "basic");
        assert_eq!(client.models().await.unwrap()[0].id, "4p-x");

        let reqs = served.join().unwrap();
        assert!(reqs[0].starts_with("GET /v3/key "), "{}", reqs[0]);
        assert_eq!(header(&reqs[0], "authorization"), Some("Bearer SECRETKEY"));
        assert!(reqs[1].starts_with("GET /v3/models "), "{}", reqs[1]);
        assert_eq!(header(&reqs[1], "authorization"), Some("Bearer SECRETKEY"));
    }

    /// `redeem` and `health` are the unauthenticated endpoints — a buyer calls
    /// them before they hold a key. Sending one would leak it needlessly.
    #[tokio::test]
    async fn redeem_and_health_send_no_authorization_header() {
        let (base, served) = mock_http(vec![
            (
                "200 OK",
                r#"{"key":"K","key_last4":"cdef","plan":"basic","expires_at":"2026-08-04","extended":false}"#.into(),
            ),
            ("200 OK", r#"{"status":"ok","models":["4p-x"]}"#.into()),
        ]);
        let r = redeem(&base, "", " CODE ", Some(" "), None).await.unwrap();
        assert_eq!(r.key.as_deref(), Some("K"));
        assert_eq!(health(&base, "").await.unwrap().status, "ok");

        let reqs = served.join().unwrap();
        assert!(reqs[0].starts_with("POST /v3/redeem "), "{}", reqs[0]);
        assert!(header(&reqs[0], "authorization").is_none());
        // Blank optionals are dropped, and the code is trimmed.
        assert!(reqs[0].contains(r#""code":"CODE""#), "{}", reqs[0]);
        assert!(!reqs[0].contains("email"), "{}", reqs[0]);
        assert!(reqs[1].starts_with("GET /healthz "), "{}", reqs[1]);
        assert!(header(&reqs[1], "authorization").is_none());
    }

    /// End-to-end through a mock SOCKS5 proxy: greeting → no-auth → CONNECT
    /// (domain ATYP, i.e. DNS resolved BY the proxy — `socks5h`) → success →
    /// plain HTTP inside the tunnel. Locks in that socks support is actually
    /// compiled into reqwest — without the `socks` feature this whole path
    /// fails at `Proxy::all` and the setting would silently be unusable.
    #[tokio::test]
    async fn socks5h_proxy_tunnels_the_request_and_resolves_dns_remotely() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let served = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 512];
            let n = s.read(&mut buf).unwrap();
            assert!(
                n >= 2 && buf[0] == 0x05,
                "not a SOCKS5 greeting: {:?}",
                &buf[..n]
            );
            s.write_all(&[0x05, 0x00]).unwrap(); // no-auth accepted
            let n = s.read(&mut buf).unwrap();
            assert!(n >= 4 && buf[1] == 0x01, "not a CONNECT: {:?}", &buf[..n]);
            // ATYP 0x03 = domain name: the client did NOT resolve locally.
            assert_eq!(buf[3], 0x03, "expected domain ATYP, got {:#x}", buf[3]);
            s.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .unwrap();
            // From here the socket IS the tunnel — speak plain HTTP on it.
            let mut req = Vec::new();
            let mut tmp = [0u8; 1024];
            loop {
                let n = s.read(&mut tmp).unwrap();
                req.extend_from_slice(&tmp[..n]);
                if n == 0 || req.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let body = r#"{"status":"ok","models":[]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            s.write_all(resp.as_bytes()).unwrap();
            String::from_utf8_lossy(&req).to_string()
        });
        // The target host doesn't exist — the request can only succeed by
        // going through the mock proxy.
        let h = health("http://ot.invalid:8080", &format!("socks5h://{addr}"))
            .await
            .unwrap();
        assert_eq!(h.status, "ok");
        let req = served.join().unwrap();
        assert!(req.starts_with("GET /healthz HTTP/1.1"), "{req}");
    }

    /// Plain HTTP proxying is the same request with an absolute URI in the
    /// request line — `mock_http` can play the proxy directly.
    #[tokio::test]
    async fn http_proxy_receives_the_absolute_form_request() {
        let (proxy_base, served) =
            mock_http(vec![("200 OK", r#"{"status":"ok","models":[]}"#.into())]);
        let h = health("http://ot.invalid:8080", &proxy_base).await.unwrap();
        assert_eq!(h.status, "ok");
        let reqs = served.join().unwrap();
        assert!(
            reqs[0].starts_with("GET http://ot.invalid:8080/healthz"),
            "expected an absolute-form request line, got: {}",
            reqs[0]
        );
    }

    /// A non-2xx surfaces the server's `error` message and the `Retry-After` hint.
    #[tokio::test]
    async fn http_error_carries_the_server_message() {
        let (base, served) = mock_http(vec![(
            "429 Too Many Requests",
            r#"{"error":"rate limited"}"#.into(),
        )]);
        let client = ApiClient::new(NativeApiProvider::Original, &base, "K", "").unwrap();
        let err = client.key_status().await.unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("429"), "{msg}");
        assert!(msg.contains("rate limited"), "{msg}");
        let _ = served.join();
    }

    #[test]
    fn flya_digest_matches_the_documented_fnv1a64_shape() {
        let events = vec![serde_json::json!({
            "type": "start_game",
            "names": ["", "", "", ""],
            "seed": null,
        })];
        assert_eq!(flya_rule_line(&events).unwrap(), "riichi4p");
        assert_eq!(
            flya_state_digest(&events).unwrap(),
            "fnv1a64:09bacf425e668fbe"
        );
    }

    #[test]
    fn flya_selected_action_maps_to_mjai_and_keeps_top_three() {
        let events = vec![
            serde_json::json!({
                "type": "start_game",
                "names": ["", "", "", ""],
                "seed": null,
            }),
            serde_json::json!({
                "type": "dahai",
                "actor": 1,
                "pai": "3m",
                "tsumogiri": false,
            }),
        ];
        let selected = serde_json::json!({
            "type": "pon",
            "pai": "3m",
            "consumed": ["3m", "3m"],
        });
        let response = FlyaDecisionResponse {
            protocol: "flya-test-api-v1".into(),
            model_id: "flya-manplus-1".into(),
            attempt: FlyaAttempt {
                status: "success".into(),
                selected_action_id: Some(7),
                action: Some(selected.clone()),
                actions: vec![
                    FlyaActionCandidate {
                        action_id: 7,
                        action: selected,
                        probability: 0.6,
                    },
                    FlyaActionCandidate {
                        action_id: 2,
                        action: serde_json::json!({
                            "type": "pass_all",
                        }),
                        probability: 0.3,
                    },
                    FlyaActionCandidate {
                        action_id: 9,
                        action: serde_json::json!({
                            "type": "dahai",
                            "pai": "9s",
                            "tsumogiri": false,
                        }),
                        probability: 0.1,
                    },
                ],
                error: None,
            },
        };
        let converted = flya_response_to_react(response, None, 0, &events).unwrap();
        assert_eq!(converted.model.as_deref(), Some("flya-manplus-1"));
        assert_eq!(converted.candidates.len(), 3);
        assert_eq!(converted.candidates[0].action, "pon");
        assert_eq!(converted.reaction.unwrap()["type"], "pon");
    }

    /// v2: a call answering the dealer's opening discard must resolve its
    /// target from the `dealer_opening_dahai` event, not only plain `dahai`.
    #[test]
    fn flya_call_targets_dealer_opening_dahai() {
        let events = vec![
            serde_json::json!({
                "type": "start_game",
                "names": ["", "", "", ""],
                "seed": null,
            }),
            serde_json::json!({
                "type": "dealer_opening",
                "actor": 1,
                "pai": "?",
            }),
            serde_json::json!({
                "type": "dealer_opening_dahai",
                "actor": 1,
                "pai": "3m",
            }),
        ];
        let selected = serde_json::json!({
            "type": "pon",
            "pai": "3m",
            "consumed": ["3m", "3m"],
        });
        let response = FlyaDecisionResponse {
            protocol: "flya-test-api-v1".into(),
            model_id: "flya-manplus-1".into(),
            attempt: FlyaAttempt {
                status: "success".into(),
                selected_action_id: Some(7),
                action: Some(selected.clone()),
                actions: vec![FlyaActionCandidate {
                    action_id: 7,
                    action: selected,
                    probability: 1.0,
                }],
                error: None,
            },
        };
        let converted = flya_response_to_react(response, None, 0, &events).unwrap();
        let reaction = converted.reaction.unwrap();
        assert_eq!(reaction["type"], "pon");
        assert_eq!(reaction["target"], 1);
    }

    /// Official digest vectors from FlyAPI-main/examples/state-digest-vectors.json
    /// — proves our canonicalization is byte-exact, so the server-side
    /// `state_digest_mismatch` pre-runtime check can never trip on our input.
    #[test]
    fn flya_digest_matches_official_vectors() {
        assert_eq!(flya_state_digest(&[]).unwrap(), "fnv1a64:09612b07b5ecb5a5");
        // decision-4p-chi.request.json /state/events
        let hidden = vec!["?"; 13];
        let events = vec![
            serde_json::json!({
                "type": "start_game",
                "names": ["seat-0", "player", "seat-2", "seat-3"],
                "seed": null,
            }),
            serde_json::json!({
                "type": "start_kyoku",
                "bakaze": "E",
                "dora_marker": "4p",
                "kyoku": 1,
                "honba": 0,
                "kyotaku": 0,
                "oya": 0,
                "scores": [25000, 25000, 25000, 25000],
                "tehais": [
                    hidden.clone(),
                    ["1m", "2m", "5m", "6m", "7m", "2p", "3p", "4p", "6s", "7s", "8s", "E", "E"],
                    hidden.clone(),
                    hidden.clone(),
                ],
            }),
            serde_json::json!({ "type": "tsumo", "actor": 0, "pai": "?" }),
            serde_json::json!({ "type": "dahai", "actor": 0, "pai": "3m", "tsumogiri": false }),
        ];
        assert_eq!(
            flya_state_digest(&events).unwrap(),
            "fnv1a64:426dd201c8cc1ed4"
        );
    }

    /// The official success example carries many fields our struct does not
    /// model (request_id, model_selection, rule_line, rule_profile,
    /// match_context, quota, attempt.latency_ms ...). Parsing must tolerate
    /// them — FlyAPI-main/examples/decision-success.response.json.
    #[test]
    fn flya_parses_official_success_example() {
        let raw = r#"{
          "protocol": "flya-test-api-v1",
          "request_id": "example-4p-discard-0001",
          "model_id": "flya-manplus-1",
          "model_selection": "server_default",
          "rule_line": "riichi4p",
          "rule_profile": "tenhou",
          "match_context": { "length": "unknown" },
          "attempt": {
            "model_id": "flya-manplus-1",
            "model_selection": "server_default",
            "status": "success",
            "selected_action_id": 2,
            "selected_index": 2,
            "action": { "type": "dahai", "pai": "W", "tsumogiri": false },
            "actions": [
              { "action_id": 2, "action": { "type": "dahai", "pai": "W", "tsumogiri": false }, "probability": 0.6 },
              { "action_id": 9, "action": { "type": "dahai", "pai": "8p", "tsumogiri": false }, "probability": 0.4 }
            ],
            "latency_ms": 236
          },
          "quota": {
            "key": "flyat_abcdef...1234", "status": "active", "key_kind": "paygo",
            "validity_months": 1, "activated_at": "2026-08-03T00:00:00Z",
            "consumed": "1.000", "replay": false,
            "expires_at": "2026-09-03T00:00:00Z", "expires_in_seconds": 2678400,
            "destroy_at": null, "destroy_in_seconds": null,
            "quota_total": "10000.000", "quota_used": "1.000", "quota_remaining": "9999.000"
          }
        }"#;
        let response: FlyaDecisionResponse = serde_json::from_str(raw).unwrap();
        let converted = flya_response_to_react(response, None, 0, &[]).unwrap();
        let reaction = converted.reaction.unwrap();
        assert_eq!(reaction["type"], "dahai");
        assert_eq!(reaction["pai"], "W");
        assert_eq!(converted.candidates[0].action, "dahai:W");
    }

    /// Runtime failure/timeout/abstain attempts omit the success-only fields
    /// and carry {code, message} — they must produce a clean error naming the
    /// server-side cause, not a serde parse failure. Shapes from
    /// FlyAPI-main/examples/decision-{failure,timeout,abstain}.response.json.
    #[test]
    fn flya_non_success_attempts_bail_with_server_cause() {
        let cases = [
            (
                r#"{"status":"failure","error":{"code":"runtime_unreachable","message":"runtime group unreachable"},"latency_ms":31,"model_id":"flya-manplus-1","model_selection":"explicit"}"#,
                "failure",
                "runtime_unreachable",
            ),
            (
                r#"{"status":"timeout","error":{"code":"runtime_timeout","message":"runtime request timed out"},"latency_ms":8000,"model_id":"flya-manout-1-3p","model_selection":"server_default"}"#,
                "timeout",
                "runtime_timeout",
            ),
            (
                r#"{"status":"abstain","latency_ms":45,"model_id":"flya-hayman-1","model_selection":"explicit"}"#,
                "abstain",
                "abstain",
            ),
        ];
        for (attempt, status, needle) in cases {
            let raw = format!(
                r#"{{"protocol":"flya-test-api-v1","request_id":"x","model_id":"flya-manplus-1","model_selection":"explicit","attempt":{attempt},"quota":{{}}}}"#
            );
            let response: FlyaDecisionResponse = serde_json::from_str(&raw).unwrap();
            let err = flya_response_to_react(response, None, 0, &[]).unwrap_err();
            let msg = format!("{err}");
            assert!(msg.contains(status), "{msg}");
            assert!(msg.contains(needle), "{msg}");
        }
    }

    #[tokio::test]
    async fn flya_posts_beta_v1_decision_with_bearer_auth() {
        let (base, served) = mock_http(vec![(
            "200 OK",
            r#"{"protocol":"flya-test-api-v1","model_id":"flya-manplus-1","attempt":{"status":"success","selected_action_id":1,"selected_index":0,"action":{"type":"dahai","pai":"5p","tsumogiri":false},"actions":[{"action_id":1,"action":{"type":"dahai","pai":"5p","tsumogiri":false},"probability":1.0}]}}"#.into(),
        )]);
        let client = ApiClient::new(NativeApiProvider::Flya, &base, "SECRETKEY", "").unwrap();
        assert!(client.is_flya());
        let converted = client
            .react(
                Some("flya-manplus-1"),
                0,
                vec![serde_json::json!({
                    "type": "start_game",
                    "names": ["", "", "", ""],
                    "seed": null,
                })],
            )
            .await
            .unwrap();
        assert_eq!(converted.reaction.unwrap()["type"], "dahai");
        let request = &served.join().unwrap()[0];
        assert!(request.starts_with("POST /beta/v1/decision "), "{request}");
        assert_eq!(header(request, "authorization"), Some("Bearer SECRETKEY"));
        assert!(request.contains("flya-mahjong-events-v2"), "{request}");
        assert!(!request.lines().next().unwrap().contains("SECRETKEY"));
    }

    #[tokio::test]
    async fn flya_activates_a_frozen_key_and_replays_the_same_request() {
        let (base, served) = mock_http(vec![
            ("403 Forbidden", r#"{"error":"test_api_key_frozen"}"#.into()),
            (
                "200 OK",
                r#"{"status":"active","key_kind":"paygo","expires_at":"2026-09-02T00:00:00Z"}"#.into(),
            ),
            (
                "200 OK",
                r#"{"protocol":"flya-test-api-v1","model_id":"flya-manplus-1","attempt":{"status":"success","selected_action_id":1,"action":{"type":"dahai","pai":"5p","tsumogiri":false},"actions":[{"action_id":1,"action":{"type":"dahai","pai":"5p","tsumogiri":false},"probability":1.0}]}}"#.into(),
            ),
        ]);
        let client = ApiClient::new(NativeApiProvider::Flya, &base, "SECRETKEY", "").unwrap();
        client
            .react(
                Some("flya-manplus-1"),
                0,
                vec![serde_json::json!({
                    "type": "start_game",
                    "names": ["", "", "", ""],
                    "seed": null,
                })],
            )
            .await
            .unwrap();
        let requests = served.join().unwrap();
        assert!(requests[0].starts_with("POST /beta/v1/decision "));
        assert!(requests[1].starts_with("GET /beta/v1/quota "));
        assert!(requests[2].starts_with("POST /beta/v1/decision "));
        let first_body = requests[0].split("\r\n\r\n").nth(1).unwrap_or_default();
        let retry_body = requests[2].split("\r\n\r\n").nth(1).unwrap_or_default();
        assert_eq!(first_body, retry_body);
    }

    #[tokio::test]
    async fn flya_forbidden_error_preserves_server_code_and_message() {
        let (base, served) = mock_http(vec![(
            "403 Forbidden",
            r#"{"error":"api_key_revoked","message":"this key was revoked"}"#.into(),
        )]);
        let client = ApiClient::new(NativeApiProvider::Flya, &base, "SECRETKEY", "").unwrap();
        let err = client
            .react(
                None,
                0,
                vec![serde_json::json!({
                    "type": "start_game",
                    "names": ["", "", "", ""],
                    "seed": null,
                })],
            )
            .await
            .unwrap_err();
        let message = format!("{err:#}");
        assert!(message.contains("api_key_revoked"), "{message}");
        assert!(message.contains("this key was revoked"), "{message}");
        assert_eq!(served.join().unwrap().len(), 1);
    }
}
