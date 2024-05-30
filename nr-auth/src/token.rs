use chrono::{DateTime, Utc};

pub type AccessToken = String;

#[derive(Clone, Debug, PartialEq)]
pub enum TokenType {
    Bearer,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    expires_at: DateTime<Utc>,
    access_token: AccessToken,
    token_type: TokenType,
}

impl Token {
    pub fn new(
        access_token: AccessToken,
        token_type: TokenType,
        expires_at: DateTime<Utc>,
    ) -> Self {
        Token {
            access_token,
            token_type,
            expires_at,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at.lt(&Utc::now())
    }

    pub fn access_token(&self) -> &AccessToken {
        &self.access_token
    }

    pub fn token_type(&self) -> &TokenType {
        &self.token_type
    }
}

#[cfg(test)]
mod test {
    use crate::token::{AccessToken, Token, TokenType};
    use chrono::{Duration, Utc};

    #[test]
    fn token_is_expired() {
        let past = Utc::now() - Duration::milliseconds(10);
        let token = Token::new(AccessToken::from("some-token"), TokenType::Bearer, past);
        assert!(token.is_expired())
    }

    #[test]
    fn token_is_not_expired() {
        let future = Utc::now() + Duration::milliseconds(10);
        let token = Token::new(AccessToken::from("some-token"), TokenType::Bearer, future);
        assert!(!token.is_expired())
    }
}
