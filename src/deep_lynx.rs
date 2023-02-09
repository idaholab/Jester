use json;
use jwt::{Claims, Header, Token, Unverified};
use reqwest;
use reqwest::blocking::multipart;
use reqwest::header;
use std::fs;
use std::io;
use std::num::ParseIntError;
use std::path::PathBuf;
use thiserror::Error;
use tokio::task;
use tokio::task::JoinError;

#[derive(Error, Debug)]
pub enum APIError {
    #[error("unknown API error")]
    Unknown,
    #[error("reqwest")]
    ReqwestError(#[from] reqwest::Error),
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
}

#[derive(Debug, Clone)]
pub struct DeepLynxAPI {
    client: reqwest::Client,
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
        let client = reqwest::Client::new();

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
        let mut headers = header::HeaderMap::new();

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

        // TODO: handle the unwraps better, I don't like the chance of panicking here
        headers.insert("x-api-key", api_key.parse().unwrap());
        headers.insert("x-api-secret", api_secret.parse().unwrap());
        headers.insert("expiry", "12h".parse().unwrap()); // keep it a short time
        let server = &self.server; // do this because format! is kinda bad at accessing fields

        let token = match self
            .client
            .get(format!("{server}/oauth/token"))
            .headers(headers)
            .send()
            .await?
            .text()
            .await
        {
            Ok(tok) => tok,
            Err(error) => {
                return Err(APIError::ReqwestError(error));
            } // Don't Panic!
        };

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

    pub async fn get(&mut self, route: &str) -> Result<String, Box<dyn std::error::Error>> {
        let mut headers = header::HeaderMap::new();

        if self.token_expired()? && self.secured {
            self.get_token().await?
        }

        match &self.bearer_token {
            None => {} // no bearer token = no attachment but also no error
            Some(t) => {
                headers.insert("Authorization", format!("Bearer {}", t).parse().unwrap());
            }
        }

        let result = match self
            .client
            .get(format!("{}/{route}", self.server))
            .headers(headers)
            .send()
            .await
            .expect("no response")
            .text()
            .await
        {
            Ok(result) => result,
            Err(error) => {
                return Err(Box::new(error));
            } // Don't Panic!
        };
        Ok(result)
    }

    pub async fn post(
        &mut self,
        route: &str,
        body: String,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let mut headers = header::HeaderMap::new();

        if self.token_expired()? && self.secured {
            self.get_token().await?
        }

        match &self.bearer_token {
            None => {} // no bearer token = no attachment but also no error
            Some(t) => {
                headers.insert("Authorization", format!("Bearer {}", t).parse().unwrap());
            }
        }

        headers.insert("Content-Type", format!("application/json").parse().unwrap());

        let result = match self
            .client
            .post(format!("{}/{route}", self.server))
            .headers(headers)
            .body(body)
            .send()
            .await
            .expect("no response")
            .text()
            .await
        {
            Ok(result) => result,
            Err(error) => {
                return Err(Box::new(error));
            } // Don't Panic!
        };

        Ok(result)
    }

    pub async fn delete(&mut self, route: &str) -> Result<String, Box<dyn std::error::Error>> {
        let mut headers = header::HeaderMap::new();
        if self.token_expired()? && self.secured {
            self.get_token().await?
        }

        match &self.bearer_token {
            None => {} // no bearer token = no attachment but also no error
            Some(t) => {
                headers.insert("Authorization", format!("Bearer {}", t).parse().unwrap());
            }
        }

        let result = match self
            .client
            .delete(format!("{}/{route}", self.server))
            .headers(headers)
            .send()
            .await
            .expect("no response")
            .text()
            .await
        {
            Ok(result) => result,
            Err(error) => {
                return Err(Box::new(error));
            } // Don't Panic!
        };
        Ok(result)
    }

    pub async fn delete_container(
        &mut self,
        container_id: u64,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let permanent = true;
        let route = format!(
            "{}/containers/{}?permanent={}",
            self.server, container_id, permanent
        );
        Ok(self.delete(&route).await?)
    }

    // hard-code specific route functions like this
    //
    pub async fn delete_data_source(
        &mut self,
        container_id: u64,
        data_source_id: u64,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let archive = false;
        let force_delete = true;
        let route = format!(
            "{}/containers/{}/import/datasources/{}?archive={}&forceDelete={}",
            self.server, container_id, data_source_id, archive, force_delete
        );
        Ok(self.delete(&route).await?)
    }

    pub async fn create_container_no_ontology(
        &mut self,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let body = format!(
            r#"
        {{
          "name": "containernamea",
          "description": "containerdesc"
        }}"#
        );
        let route = "containers".to_string();
        Ok(self.post(&route, body).await?)
    }

    pub async fn create_container(
        &mut self,
        name: String,
        description: String,
        ontology_file: String,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let mut headers = header::HeaderMap::new();
        if self.token_expired()? && self.secured {
            self.get_token().await?
        }

        match &self.bearer_token {
            None => {} // no bearer token = no attachment but also no error
            Some(t) => {
                headers.insert("Authorization", format!("Bearer {}", t).parse().unwrap());
            }
        }

        let route = "containers/import?dryrun=false";

        let file = fs::read(ontology_file).unwrap();
        let file_part = reqwest::multipart::Part::bytes(file)
            .file_name("ontology.owl")
            .mime_str("application/rdf+xml")?;

        let form = reqwest::multipart::Form::new()
            .text("name", name)
            .text("description", description)
            .text("data_versioning_enabled", "false")
            .text("path", "")
            .part("file", file_part);

        let result = self
            .client
            .post(format!("{}/{route}", self.server))
            .headers(headers)
            .multipart(form)
            .send()
            .await
            .expect("no response")
            .text()
            .await
            .unwrap();
        // .or_ok(ApiError::FailedToParse);

        let parsed = json::parse(&result)?;
        let container_id = parsed["value"].to_string().parse::<u64>()?;

        Ok(container_id)
    }

    pub async fn list_containers(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        let route = "containers".to_string();
        Ok(self.get(&route).await?)
    }

    pub async fn create_metatype(
        &mut self,
        container_id: u64,
        name: &str,
        description: &str,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let body = format!(
            r#"
        {{
            "name": "{name}",
            "description": "{description}"
        }}"#
        );
        let route = format!("containers/{container_id}/metatypes");
        let result = self.post(&route, body).await?;
        let parsed = json::parse(&result)?;
        let metatype_id = parsed["value"][0]["id"].to_string().parse::<u64>()?;
        Ok(metatype_id)
    }

    pub async fn create_relationship_pair(
        &mut self,
        container_id: u64,
        name: &str,
        origin_metatype_id: u64,
        destination_metatype_id: u64,
        relationship_id: u64,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let body = format!(
            r#"
            {{
                "name": "{name}",
                "description": "Sensor is managed by Panel",
                "origin_metatype_id": "{origin_metatype_id}",
                "destination_metatype_id": "{destination_metatype_id}",
                "relationship_id": "{relationship_id}",
                "relationship_type": "many:many"
            }}"#
        );
        let route = format!("containers/{container_id}/metatype_relationship_pairs");
        let result = self.post(&route, body).await?;
        // println!("{}",result);
        let parsed = json::parse(&result)?;
        let relationship_pair_id = parsed["value"][0]["id"].to_string().parse::<u64>()?;
        Ok(relationship_pair_id)
    }

    pub async fn create_node(
        &mut self,
        container_id: u64,
        metatype_id: u64,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let body = format!(
            r#"
            {{
                "container_id": "{container_id}",
                "metatype_id": "{metatype_id}"
            }}"#
        );
        let route = format!("containers/{container_id}/graphs/nodes");
        let result = self.post(&route, body).await.unwrap();
        let parsed = json::parse(&result).unwrap();
        let node_id = parsed["value"][0]["id"].to_string().parse::<u64>().unwrap();
        Ok(node_id)
    }

    pub async fn create_timeseries_data_source(
        &mut self,
        container_id: u64,
        name: &str,
        metatype_id: u64,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let body = format!(
            r#"
        {{
            "container_id": "{container_id}",
            "name": "{name}",
            "adapter_type": "timeseries",
            "active": true,
            "config": {{
                "chunk_interval": "1000",
                "kind": "timeseries",
                "columns": [
                    {{
                        "is_primary_timestamp": true,
                        "unique": true,
                        "id": "7af5a05b-67d8-491e-969c-f7ce8928d939",
                        "type": "number64",
                        "column_name": "time_index",
                        "property_name": "time_index",
                        "date_conversion_format_string": "YYYY-MM-DD HH24:MI:SS.US"
                    }},
                    {{
                        "is_primary_timestamp": false,
                        "unique": false,
                        "id": "d6aabdd5-6015-4cd1-a6a9-08989dd9118f",
                        "type": "float64",
                        "column_name": "measurment",
                        "property_name": "measurment_name"
                    }}
                ],
                "attachment_parameters": [
                    {{
                        "key": "",
                        "type": "metatype_id",
                        "value": "{metatype_id}",
                        "operator": "=="
                    }}
                ]
            }}
        }}"#
        );
        let route = format!("containers/{container_id}/import/datasources");
        let result = self.post(&route, body).await.unwrap();
        let parsed = json::parse(&result).unwrap();
        let data_source_id = parsed["value"]["id"].to_string().parse::<u64>().unwrap();
        Ok(data_source_id)
    }

    pub async fn create_daq_timeseries_data_source(
        &mut self,
        container_id: u64,
        name: &str,
        metatype_id: u64,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let body = format!(
            r#"
        {{
            "container_id": "{container_id}",
            "name": "{name}",
            "adapter_type": "timeseries",
            "active": true,
            "config": {{
                "chunk_interval": "1000",
                "kind": "timeseries",
                "columns": [
                    {{
                        "is_primary_timestamp": true,
                        "unique": true,
                        "id": "7af5a05b-67d8-491e-969c-f7ce8928d939",
                        "type": "number64",
                        "column_name": "time_index",
                        "property_name": "time_index",
                        "date_conversion_format_string": "YYYY-MM-DD HH24:MI:SS.US"
                    }},
                    {{
                        "is_primary_timestamp": false,
                        "unique": false,
                        "id": "d6aabdd5-6015-4cd1-a6a9-08989dd9118f",
                        "type": "float64",
                        "column_name": "s0",
                        "property_name": "measurment_s0"
                    }},
                    {{
                        "is_primary_timestamp": false,
                        "unique": false,
                        "id": "d6aabdd5-6015-4cd1-a6a9-08989dd9118f",
                        "type": "float64",
                        "column_name": "s1",
                        "property_name": "measurment_s1"
                    }},
                    {{
                        "is_primary_timestamp": false,
                        "unique": false,
                        "id": "d6aabdd5-6015-4cd1-a6a9-08989dd9118f",
                        "type": "float64",
                        "column_name": "s2",
                        "property_name": "measurment_s2"
                    }},
                    {{
                        "is_primary_timestamp": false,
                        "unique": false,
                        "id": "d6aabdd5-6015-4cd1-a6a9-08989dd9118f",
                        "type": "float64",
                        "column_name": "s3",
                        "property_name": "measurment_s3"
                    }},
                    {{
                        "is_primary_timestamp": false,
                        "unique": false,
                        "id": "d6aabdd5-6015-4cd1-a6a9-08989dd9118f",
                        "type": "float64",
                        "column_name": "s4",
                        "property_name": "measurment_s4"
                    }},
                    {{
                        "is_primary_timestamp": false,
                        "unique": false,
                        "id": "d6aabdd5-6015-4cd1-a6a9-08989dd9118f",
                        "type": "float64",
                        "column_name": "s5",
                        "property_name": "measurment_s5"
                    }}
                ],
                "attachment_parameters": [
                    {{
                        "key": "",
                        "type": "metatype_id",
                        "value": "{metatype_id}",
                        "operator": "=="
                    }}
                ]
            }}
        }}"#
        );
        let route = format!("containers/{container_id}/import/datasources");
        let result = self.post(&route, body).await.unwrap();
        let parsed = json::parse(&result).unwrap();
        let data_source_id = parsed["value"]["id"].to_string().parse::<u64>().unwrap();
        Ok(data_source_id)
    }

    // TODO: fix this once the rest of the package is normalized
    pub async fn import(
        &mut self,
        container_id: u64,
        data_source_id: u64,
        file: Option<PathBuf>,
        data: Option<Vec<u8>>,
    ) -> Result<(), APIError> {
        let mut headers = header::HeaderMap::new();
        if (self.secured && self.bearer_token.is_none()) || (self.token_expired()? && self.secured)
        {
            self.get_token().await?;
        }

        match &self.bearer_token {
            None => {} // no bearer token = no attachment but also no error
            Some(t) => {
                headers.insert("Authorization", format!("Bearer {}", t).parse().unwrap());
            }
        }

        let server = &self.server;
        let route = format!(
            "{server}/containers/{container_id}/import/datasources/{data_source_id}/imports"
        );

        let res = task::spawn_blocking(move || {
            // we have to use a blocking client in case we've got a file
            let client = reqwest::blocking::Client::new().post(&route);

            // TODO: handle responses better
            match file {
                None => {}
                Some(p) => {
                    let form = match multipart::Form::new().file("file", p) {
                        Ok(f) => f,
                        Err(e) => {
                            panic!()
                        }
                    };
                    match client.headers(headers.clone()).multipart(form).send() {
                        Ok(_) => {}
                        Err(e) => panic!(),
                    }
                }
            }

            let client = reqwest::blocking::Client::new().post(&route);
            match data {
                None => {}
                Some(p) => match client.headers(headers.clone()).json(&p).send() {
                    Ok(_) => {}
                    Err(e) => panic!(),
                },
            }
        })
        .await?;

        Ok(())
    }

    pub async fn create_manual_import(
        &mut self,
        container_id: u64,
        data_source_id: u64,
        name: String,
        description: String,
        import_file: String,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let mut headers = header::HeaderMap::new();
        if self.token_expired()? && self.secured {
            self.get_token().await?
        }

        match &self.bearer_token {
            None => {} // no bearer token = no attachment but also no error
            Some(t) => {
                headers.insert("Authorization", format!("Bearer {}", t).parse().unwrap());
            }
        }

        let route =
            format!("containers/{container_id}/import/datasources/{data_source_id}/imports");

        let file = fs::read(&import_file).unwrap();
        let file_part = reqwest::multipart::Part::bytes(file)
            .file_name(import_file)
            .mime_str("text/csv")?;

        let form = reqwest::multipart::Form::new()
            .text("name", name)
            .text("description", description)
            .text("data_versioning_enabled", "false")
            .text("path", "")
            .part("file", file_part);

        let result = self
            .client
            .post(format!("{}/{route}", self.server))
            .headers(headers)
            .multipart(form)
            .send()
            .await
            .expect("no response")
            .text()
            .await
            .unwrap();
        // .or_ok(ApiError::FailedToParse);

        let parsed = json::parse(&result)?;
        let container_id = parsed["value"].to_string().parse::<u64>()?;

        Ok(container_id)
    }

    pub async fn create_relationship(
        &mut self,
        container_id: u64,
        name: &str,
        desc: &str,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let body = format!(
            r#"
            {{
                "name": "{name}",
                "description": "{desc}"
            }}"#
        );
        let route = format!("containers/{container_id}/metatype_relationships");
        let result = self.post(&route, body).await.unwrap();
        let parsed = json::parse(&result).unwrap();
        let relationship_id = parsed["value"][0]["id"].to_string().parse::<u64>().unwrap();
        Ok(relationship_id)
    }

    pub async fn create_edge(
        &mut self,
        container_id: u64,
        origin_id: u64,
        destination_id: u64,
        relationship_pair_id: u64,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let body = format!(
            r#"
                {{
                    "container_id": "{container_id}",
                    "origin_id": "{origin_id}",
                    "destination_id": "{destination_id}",
                    "relationship_pair_id": "{relationship_pair_id}",
                    "modified_at": "will attempt to update edge if exists",
                    "properties": {{}}
                }}"#
        );
        let route = format!("containers/{container_id}/graphs/edges");
        let result = self.post(&route, body).await?;
        let parsed = json::parse(&result)?;
        let edge_id = parsed["value"][0]["id"].to_string().parse::<u64>()?;
        Ok(edge_id)
    }
} // impl Api
