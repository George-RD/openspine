fn candidate_error_is_invalid_token(err: &teloxide::RequestError) -> bool {
    matches!(
        err,
        teloxide::RequestError::Api(teloxide::ApiError::InvalidToken)
    )
}

use super::TelegramConnector;
use teloxide::prelude::*;

impl TelegramConnector {
    /// Validate a candidate token without mutating the live credential. API
    /// `InvalidToken` is a confirmed rejection; all other request failures
    /// remain operational errors so admission records a failed connector call.
    pub async fn validate_candidate_token_id(
        &self,
        candidate: &str,
    ) -> Result<Option<i64>, anyhow::Error> {
        let api_url = self.bot.lock().api_url.clone();
        let mut bot = Bot::new(candidate.to_string());
        if let Some(url) = api_url {
            bot = bot.set_api_url(url);
        }
        match bot.get_me().send().await {
            Ok(user) => Ok(Some(user.id.0 as i64)),
            Err(err) if candidate_error_is_invalid_token(&err) => Ok(None),
            Err(err) => Err(anyhow::anyhow!("telegram getMe transport failed: {err}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_invalid_token_api_error_is_confirmed_rejection() {
        let error = teloxide::RequestError::Api(teloxide::ApiError::InvalidToken);
        assert!(candidate_error_is_invalid_token(&error));
    }
}
