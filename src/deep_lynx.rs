use crate::deep_lynx::APIError::MissingFields;
use jwt::{Claims, Header, Token, Unverified};
use multipart::client::lazy::Multipart;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::num::ParseIntError;
use std::path::PathBuf;
use thiserror::Error;
use tokio::task::JoinError;
use ureq;

#[derive(Error, Debug)]
pub enum APIError {
    #[error("unknown API error")]
    Unknown,
    #[error("missing required fields")]
    MissingFields(Option<Vec<String>>),
    #[error("jwt parse")]
    JWTParsingError(#[from] jwt::error::Error),
    #[error("missing iat claim")]
    MissingIATError,
    #[error("parse int error")]
    ParseIntError(#[from] ParseIntError),
    #[error("io error")]
    IOError(#[from] io::Error),
    #[error("join error")]
    Join(#[from] JoinError),
    #[error("ureq error")]
    UreqError(#[from] ureq::Error),
}

#[derive(Debug, Clone)]
pub struct DeepLynxAPI {
    client: ureq::Agent,
    server: String,
    bearer_token: Option<String>,
    api_key: Option<String>,
    api_secret: Option<String>,
    secured: bool,
}

impl DeepLynxAPI {
    pub async fn new(
        server: String,
        api_key: Option<String>,
        api_secret: Option<String>,
    ) -> Result<DeepLynxAPI, APIError> {
        let client = ureq::AgentBuilder::new().build();

        Ok(DeepLynxAPI {
            client,
            server,
            secured: api_key.is_some() && api_secret.is_some(),
            api_key,
            api_secret,
            bearer_token: None,
        })
    }

    // get token should be called by the root GET/POST/PUT etc. and only if the JWT has expired
    async fn get_token(&mut self) -> Result<(), APIError> {
        let api_key = match &self.api_key {
            None => return Err(APIError::MissingFields(Some(vec!["api_key".to_string()]))),
            Some(k) => k,
        };

        let api_secret = match &self.api_secret {
            None => {
                return Err(APIError::MissingFields(Some(
                    vec!["api_secret".to_string()],
                )))
            }
            Some(k) => k,
        };

        let server = &self.server; // do this because format! is kinda bad at accessing fields
        let token = self
            .client
            .get(format!("{server}/oauth/token").as_str())
            .set("x-api-key", api_key)
            .set("x-api-secret", api_secret)
            .set("expiry", "12h")
            .call()?
            .into_string()?;

        // remove quotes from token text()
        let token = &token[1..token.len() - 1];
        self.bearer_token = Some(token.to_string());

        Ok(())
    }

    fn token_expired(&self) -> Result<bool, APIError> {
        match &self.bearer_token {
            None => Ok(false), // return false because it might be we're calling unsecured here
            Some(token) => {
                let parsed: Token<Header, Claims, Unverified> =
                    jwt::Token::parse_unverified(token)?;

                let claims = parsed.claims();

                let exp = match claims.registered.expiration {
                    None => return Err(APIError::MissingIATError),
                    Some(id) => id,
                };

                let current_time = chrono::Utc::now().timestamp();

                Ok(exp <= current_time as u64)
            }
        }
    }

    // TODO: fix this once the rest of the package is normalized
    pub async fn import(
        &mut self,
        container_id: u64,
        data_source_id: u64,
        file: Option<PathBuf>,
        data: Option<Vec<u8>>,
    ) -> Result<(), APIError> {
        if (self.secured && self.bearer_token.is_none()) || (self.token_expired()? && self.secured)
        {
            self.get_token().await?;
        }

        let server = &self.server;
        let route = format!(
            "{server}/containers/{container_id}/import/datasources/{data_source_id}/imports"
        );
        let mut agent = self.client.post(route.as_str());

        match &self.bearer_token {
            None => {} // no bearer token = no attachment but also no error
            Some(t) => agent = agent.set("Authorization", format!("Bearer {t}").as_str()),
        }

        match file {
            None => {}
            Some(f) => {
                let guess = match new_mime_guess::from_path(f.clone()).first() {
                    None => return Err(APIError::Unknown),
                    Some(m) => m,
                };

                let opened = BufReader::new(File::open(f.clone())?);

                let mut m = Multipart::new();
                m.add_text("key", "value");
                m.add_stream("data", opened, f.to_str(), Some(guess));

                let mdata = m.prepare().unwrap();
                agent
                    .set(
                        "Content-Type",
                        &format!("multipart/form-data; boundary={}", mdata.boundary()),
                    )
                    .send(mdata)?;

                return Ok(());
            }
        }

        match data {
            None => {}
            Some(d) => {}
        }

        Err(MissingFields(None))
    }
} // impl Api
