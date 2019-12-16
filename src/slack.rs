use std::error::Error as StdError;

use reqwest::header::{CONTENT_TYPE, AUTHORIZATION};
use serde::{Serialize, Deserialize};
use std::fmt::{self, Display};
use warp::{http::{StatusCode}, Rejection, Reply};

const SLACK_URL: &str = "https://slack.com/api/";

#[derive(Debug)]
pub enum SlackError {
    HttpError(String)
}

impl std::convert::From<reqwest::Error> for SlackError {
    fn from(_error: reqwest::Error) -> Self {
        // TODO: properly implement this
        SlackError::HttpError(String::from("Reqwest Error"))
    }
}
impl std::convert::From<SlackError> for Rejection {
    fn from(err: SlackError) -> Rejection {
        warp::reject::custom(err)
    }
}

impl Display for SlackError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            SlackError::HttpError(s) => s,
        })
    }
}
impl StdError for SlackError {}

#[derive(Clone)]
pub struct Slack {
    pub api_key: String
}
#[derive(Serialize, Deserialize, Debug)]
pub struct MessageResponse {
    pub ok: bool,
    pub ts: String,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct SlackMessage {
    pub channel: String,
    pub text: String,
    pub thread_ts: Option<String>, // TODO: Make this better typed
    pub as_user: bool
}
impl Slack {
    pub fn send_message(&self, message: &SlackMessage) -> Result<MessageResponse, SlackError> {
        let client = reqwest::Client::new();
        let url = format!("{}/chat.postMessage", SLACK_URL);
        let msg_response: MessageResponse = client.post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", &self.api_key))
            .header(CONTENT_TYPE, "application/json")
            .json(&message)
            .send()?
            .json()?;

        Ok(msg_response)
    }
}


