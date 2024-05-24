use crate::token::{AccessToken, Token, TokenType};
use crate::{TokenRetriever, TokenRetrieverError};
use chrono::DateTime;

// TODO: #[derive(Default)] until functionality is implemented. It might not be necessary later
#[derive(Default)]
pub struct TokenRetrieverDefault {}

impl TokenRetriever for TokenRetrieverDefault {
    fn retrieve(&self) -> Result<Token, TokenRetrieverError> {
        //TODO
        Ok(Token::new(
            AccessToken::default(),
            TokenType::Bearer,
            DateTime::default(),
        ))
    }
}
