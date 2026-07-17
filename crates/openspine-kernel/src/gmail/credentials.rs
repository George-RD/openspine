//! Credential validation for the Gmail connector (D-014 secret-intake).
//!
//! Extracted from `gmail.rs` to stay under the 500-line module limit.
//! Contains the paired OAuth credential validation used by the secret
//! intake capture flow.

use super::GmailConnector;

impl GmailConnector {
    /// Validate a candidate OAuth credential pair by attempting a single
    /// token refresh POST with the supplied client secret and refresh
    /// token.  Neither value touches the live vault slots or the cached
    /// access token — this is a pure probe that returns `true` iff Google
    /// accepts the pair.
    ///
    /// The caller (secret-intake capture) determines which argument is
    /// which based on slot names.
    pub async fn validate_credential_pair(&self, client_secret: &str, refresh_token: &str) -> bool {
        let Ok(resp) = self
            .http
            .post(&self.token_url)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", client_secret),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
        else {
            return false;
        };
        resp.status().is_success()
    }
}
