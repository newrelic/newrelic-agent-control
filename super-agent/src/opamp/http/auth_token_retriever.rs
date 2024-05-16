use nr_auth::token_retriever::TokenRetrieverDefault;
use nr_auth::TokenRetriever;

#[derive(thiserror::Error, Debug)]
pub enum TokenRetrieverBuilderError {
    #[error("errors building the TokenRetriever: `{0}`")]
    BuildingError(String),
}

/// TokenRetrieverBuilder is responsible for building TokenRetrievers.
/// TokenRetrievers ownership is passed to the HTTP Client (send+sync+static)
/// so having a builder makes possible to create TokenRetrievers and deal
/// with lifetimes.
/// Having the trait makes testing flexible.
pub trait TokenRetrieverBuilder {
    type TokenRetriever: TokenRetriever + Send + Sync + 'static;

    /// Build the Token Retriever
    fn build(&self) -> Result<Self::TokenRetriever, TokenRetrieverBuilderError>;
}

/// Default TokenRetriever builder. Not implemented yet.
pub struct TokenRetrieverBuilderDefault;

impl TokenRetrieverBuilder for TokenRetrieverBuilderDefault {
    type TokenRetriever = TokenRetrieverDefault;

    fn build(&self) -> Result<Self::TokenRetriever, TokenRetrieverBuilderError> {
        Ok(TokenRetrieverDefault::default())
    }
}

#[cfg(test)]
pub mod test {
    use chrono::Utc;
    use fake::faker::lorem::en::Word;
    use fake::Fake;
    use mockall::mock;

    use nr_auth::token::{AccessToken, Token, TokenType};
    use nr_auth::{TokenRetriever, TokenRetrieverError};

    use crate::opamp::http::auth_token_retriever::{
        TokenRetrieverBuilder, TokenRetrieverBuilderError,
    };

    mock! {
        pub TokenRetrieverMock {}

        impl TokenRetriever for TokenRetrieverMock{
            fn retrieve(&self) -> Result<Token, TokenRetrieverError>;
        }
    }

    impl MockTokenRetrieverMock {
        pub fn should_retrieve(&mut self, token: Token) {
            self.expect_retrieve().once().return_once(move || Ok(token));
        }

        pub fn should_return_error(&mut self, error: TokenRetrieverError) {
            self.expect_retrieve()
                .once()
                .return_once(move || Err(error));
        }
    }

    mock! {
        pub TokenRetrieverBuilderMock {}

        impl TokenRetrieverBuilder for TokenRetrieverBuilderMock{
            type TokenRetriever = MockTokenRetrieverMock;

            fn build(&self) -> Result<<MockTokenRetrieverBuilderMock as TokenRetrieverBuilder>::TokenRetriever, TokenRetrieverBuilderError>;
        }
    }

    impl MockTokenRetrieverBuilderMock {
        pub fn should_build(&mut self, token_retriever: MockTokenRetrieverMock) {
            self.expect_build()
                .once()
                .return_once(move || Ok(token_retriever));
        }

        pub fn should_fail_on_build(&mut self, error: TokenRetrieverBuilderError) {
            self.expect_build().once().return_once(move || Err(error));
        }
    }

    pub fn token_stub() -> Token {
        Token::new(
            AccessToken::from(Word().fake::<String>()),
            TokenType::Bearer,
            Utc::now(),
        )
    }
}
