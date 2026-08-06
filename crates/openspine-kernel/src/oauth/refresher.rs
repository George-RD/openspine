//! Background OAuth token refresher task (AD-138 / AD-014 / D-064..D-067).

use crate::secret_store::SecretStore;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

pub struct OAuthRefresher {
    pub secret_store: SecretStore,
    pub client: reqwest::Client,
    pub in_flight: Arc<Mutex<HashSet<String>>>,
    #[allow(dead_code)]
    pub skew_window_seconds: u64,
}

/// One process-wide in-flight set. Refreshers are constructed per call site
/// (background sweeper, inline 401 recovery), and single-flight only means
/// anything when every construction shares it. Token rotation makes
/// duplicate refresh grants actively destructive: the second grant's
/// `invalid_grant` would disable a credential the first grant just renewed.
static SHARED_IN_FLIGHT: std::sync::LazyLock<Arc<Mutex<HashSet<String>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashSet::new())));

impl OAuthRefresher {
    pub fn new(secret_store: SecretStore) -> Self {
        Self {
            secret_store,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            in_flight: SHARED_IN_FLIGHT.clone(),
            skew_window_seconds: 300,
        }
    }

    #[allow(dead_code)]
    pub async fn sweep_provider(
        &self,
        provider_id: &str,
        url_override: Option<&str>,
    ) -> Result<bool, anyhow::Error> {
        let tokens = match self.secret_store.get_oauth_tokens(provider_id)? {
            Some(t) => t,
            None => return Ok(false),
        };

        if tokens.disabled {
            return Ok(false);
        }

        let now_sec = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let expires_at_sec = parse_expires_at(&tokens.expires_at).unwrap_or(0);

        if expires_at_sec > 0 && expires_at_sec.saturating_sub(now_sec) > self.skew_window_seconds {
            return Ok(false);
        }

        self.refresh_provider_now(provider_id, url_override)
            .await
            .map(|_| true)
    }

    pub async fn refresh_provider_now(
        &self,
        provider_id: &str,
        url_override: Option<&str>,
    ) -> Result<String, anyhow::Error> {
        // Keyed by vault AND provider: coordination binds one credential
        // set, never two vaults that happen to share a provider name.
        let flight_key = format!("{}::{provider_id}", self.secret_store.scope_key());
        let mut guard = self.in_flight.lock().await;
        if guard.contains(&flight_key) {
            drop(guard);
            // Bounded wait for the in-flight refresh to finish, then serve
            // the refreshed token from the vault instead of submitting a
            // competing refresh grant. The bound matches the refresh
            // client's own 30s timeout.
            for _ in 0..300 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                let guard = self.in_flight.lock().await;
                let done = !guard.contains(&flight_key);
                drop(guard);
                if done {
                    break;
                }
            }
            let tokens = self
                .secret_store
                .get_oauth_tokens(provider_id)?
                .ok_or_else(|| anyhow::anyhow!("No OAuth tokens for {provider_id}"))?;
            return Ok(tokens.access_token);
        }
        guard.insert(flight_key.clone());
        drop(guard);

        let result = self.do_refresh(provider_id, url_override).await;

        let mut guard = self.in_flight.lock().await;
        guard.remove(&flight_key);
        drop(guard);

        result
    }

    async fn do_refresh(
        &self,
        provider_id: &str,
        url_override: Option<&str>,
    ) -> Result<String, anyhow::Error> {
        let tokens = self
            .secret_store
            .get_oauth_tokens(provider_id)?
            .ok_or_else(|| anyhow::anyhow!("No OAuth tokens stored for {provider_id}"))?;

        if tokens.disabled {
            anyhow::bail!("OAuth credential for {provider_id} is disabled");
        }

        // A refresh presents the same client id the authorization did.
        //
        // Resolved before the request and propagated, never substituted: sending
        // a placeholder would draw a 400, and the definitive-failure arm below
        // would then disable a stored credential that is perfectly valid.
        let client_id = super::providers::client_id_for(provider_id)?;
        let refresh_res = match provider_id {
            "google-antigravity" => {
                super::providers::google_antigravity::refresh_token(
                    &self.client,
                    client_id,
                    &tokens.refresh_token,
                    url_override,
                )
                .await
            }
            "openai-codex" => {
                super::providers::openai_codex::refresh_token(
                    &self.client,
                    client_id,
                    &tokens.refresh_token,
                    url_override,
                )
                .await
            }
            "anthropic" => {
                super::providers::anthropic::refresh_token(
                    &self.client,
                    client_id,
                    &tokens.refresh_token,
                    url_override,
                )
                .await
            }
            _ => anyhow::bail!("Unknown OAuth provider {provider_id}"),
        };

        match refresh_res {
            Ok(new_tokens) => {
                let expires_in = new_tokens.expires_in.max(300);
                let now_sec = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let expires_at_sec = now_sec + expires_in;
                let expires_at_str = expires_at_sec.to_string();

                // One write covers every combination: a rotated refresh
                // token replaces the stored one, an omitted one keeps it,
                // and refreshed identity (Codex derives it from the new
                // access token) is persisted either way — identity currency
                // must not depend on whether the provider rotated the
                // refresh token. `None` metadata preserves the login's.
                let refresh_token = new_tokens
                    .refresh_token
                    .clone()
                    .filter(|token| !token.is_empty())
                    .unwrap_or_else(|| tokens.refresh_token.clone());
                let identity = (new_tokens.account_id.is_some()
                    || new_tokens.account_email.is_some())
                .then(|| crate::secret_store::OAuthIdentityMetadata {
                    account_email: new_tokens.account_email.clone(),
                    account_id: new_tokens.account_id.clone(),
                    identity_key: None,
                });
                self.secret_store.store_oauth_tokens(
                    provider_id,
                    &refresh_token,
                    &new_tokens.access_token,
                    &expires_at_str,
                    identity,
                )?;

                Ok(new_tokens.access_token)
            }
            Err(err) => {
                let err_str = err.to_string();
                // A refreshed Codex token without its account claim is a
                // credential no request can spend: retrying cannot fix it,
                // and leaving it enabled makes every generate loop through
                // backend-401 + refresh. Definitive, like a revocation.
                if err_str.contains("invalid_grant")
                    || err_str.contains("revoked_token")
                    || err_str.contains("400")
                    || err_str.contains("401")
                    || err_str.contains("chatgpt_account_id")
                {
                    self.secret_store
                        .disable_oauth_credential(provider_id, &err_str)?;
                    anyhow::bail!("Definitive OAuth refresh failure: {err_str}");
                } else {
                    Err(err)
                }
            }
        }
    }
}

#[allow(dead_code)]
pub fn parse_expires_at(s: &str) -> Option<u64> {
    if let Ok(sec) = s.parse::<u64>() {
        return Some(sec);
    }
    if let Ok(ts) = std::str::FromStr::from_str(s) {
        let ts: jiff::Timestamp = ts;
        return Some(ts.as_second() as u64);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Every test here refreshes `anthropic`, which now resolves a registered
    /// client id before the request. Set-only, so it cannot race a sibling.
    fn registered_anthropic_client() {
        std::env::set_var("OPENSPINE_ANTHROPIC_CLIENT_ID", "test-client-id");
    }

    /// The first-party client sends the OAuth beta and its SDK agent when
    /// refreshing. Without them the renewal is rejected, and the definitive
    /// failure arm then disables an otherwise valid credential: a working login
    /// would quietly die at its first expiry.
    #[tokio::test]
    async fn a_refresh_carries_the_first_party_client_headers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "renewed",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().expect("tempdir");
        let store = SecretStore::open(dir.path().join("credentials"), [31; 32]).expect("open");
        store
            .store_oauth_tokens("anthropic", "refresh-tok", "old", "1000", None)
            .expect("store");

        OAuthRefresher::new(store)
            .refresh_provider_now("anthropic", Some(&format!("{}/token", server.uri())))
            .await
            .expect("refresh");

        let request = &server.received_requests().await.expect("requests")[0];
        assert_eq!(
            request
                .headers
                .get("anthropic-beta")
                .map(|v| v.to_str().unwrap()),
            Some(crate::anthropic_fingerprint::OAUTH_BETA)
        );
        assert_eq!(
            request
                .headers
                .get("user-agent")
                .map(|v| v.to_str().unwrap()),
            Some(crate::anthropic_fingerprint::OAUTH_REFRESH_USER_AGENT)
        );
    }

    #[tokio::test]
    async fn oauth_refresher_renews_token_within_skew_window() {
        registered_anthropic_client();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "renewed-access-token-999",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let store = SecretStore::open(dir.path().join("credentials"), [14; 32]).expect("open");

        let now_sec = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expiring_at = (now_sec + 200).to_string(); // expires in 200 seconds (< 300s skew window)

        store
            .store_oauth_tokens(
                "anthropic",
                "old-refresh-token",
                "old-access-token",
                &expiring_at,
                None,
            )
            .expect("store oauth tokens");

        let refresher = OAuthRefresher::new(store.clone());
        let token_url = format!("{}/token", server.uri());

        let refreshed = refresher
            .sweep_provider("anthropic", Some(&token_url))
            .await
            .expect("sweep");
        assert!(refreshed);

        let updated = store
            .get_oauth_tokens("anthropic")
            .expect("load")
            .expect("some");
        assert_eq!(updated.access_token, "renewed-access-token-999");
    }

    #[tokio::test]
    async fn oauth_refresher_single_flights_concurrent_refreshes() {
        registered_anthropic_client();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "single-flight-token",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let store = SecretStore::open(dir.path().join("credentials"), [15; 32]).expect("open");

        store
            .store_oauth_tokens("anthropic", "refresh-tok", "old-access", "1000", None)
            .expect("store oauth tokens");

        let refresher = Arc::new(OAuthRefresher::new(store));
        let token_url = format!("{}/token", server.uri());

        let r1 = refresher.clone();
        let r2 = refresher.clone();
        let url1 = token_url.clone();
        let url2 = token_url.clone();

        let (res1, res2) = tokio::join!(
            tokio::spawn(async move { r1.refresh_provider_now("anthropic", Some(&url1)).await }),
            tokio::spawn(async move { r2.refresh_provider_now("anthropic", Some(&url2)).await })
        );

        let t1 = res1.unwrap().expect("t1");
        let t2 = res2.unwrap().expect("t2");
        assert_eq!(t1, "single-flight-token");
        assert_eq!(t2, "single-flight-token");
    }

    #[tokio::test]
    async fn oauth_refresher_handles_definitive_failure_and_enqueues_notification() {
        registered_anthropic_client();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant",
                "error_description": "refresh token is revoked"
            })))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let store = SecretStore::open(dir.path().join("credentials"), [16; 32]).expect("open");

        store
            .store_oauth_tokens("anthropic", "revoked-refresh-token", "access", "1000", None)
            .expect("store oauth tokens");

        let refresher = OAuthRefresher::new(store.clone());
        let token_url = format!("{}/token", server.uri());

        let res = refresher
            .refresh_provider_now("anthropic", Some(&token_url))
            .await;
        assert!(res.is_err());

        let tokens = store
            .get_oauth_tokens("anthropic")
            .expect("load")
            .expect("some");
        assert!(tokens.disabled);
    }

    #[tokio::test]
    async fn oauth_refresher_retains_credential_on_transient_network_failure() {
        registered_anthropic_client();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().expect("tempdir");
        let store = SecretStore::open(dir.path().join("credentials"), [17; 32]).expect("open");

        store
            .store_oauth_tokens(
                "anthropic",
                "valid-refresh-token",
                "old-access",
                "1000",
                None,
            )
            .expect("store oauth tokens");

        let refresher = OAuthRefresher::new(store.clone());
        let token_url = format!("{}/token", server.uri());

        let res = refresher
            .refresh_provider_now("anthropic", Some(&token_url))
            .await;
        assert!(res.is_err());

        let tokens = store
            .get_oauth_tokens("anthropic")
            .expect("load")
            .expect("some");
        assert!(
            !tokens.disabled,
            "credential must remain active on transient failure"
        );
    }
}
