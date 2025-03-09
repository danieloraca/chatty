use async_openai::config::OpenAIConfig;
use async_openai::types::{
    ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use async_openai::Client;
use axum::body::Bytes;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::{routing::post, Json, Router};
use dotenvy::dotenv;
use futures::StreamExt;
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

#[derive(Deserialize, Serialize, Debug)]
struct SlackEvent {
    #[serde(rename = "type")]
    event_type: Option<String>,
    challenge: Option<String>,
    event: Option<SlackMessageEvent>,
    event_id: Option<String>, // For deduplication
}

#[derive(Deserialize, Serialize, Debug)]
struct SlackMessageEvent {
    text: String,
    user: String,
    channel: String,
    #[serde(rename = "type")]
    msg_type: String,
    bot_id: Option<String>,
    subtype: Option<String>,
}

#[derive(Serialize)]
struct SlackResponse {
    text: String,
    channel: String,
}

#[derive(Clone)]
struct AppState {
    client: Arc<Client<OpenAIConfig>>,
    slack_client: HttpClient,
    processed_events: Arc<tokio::sync::Mutex<HashSet<String>>>, // Track processed event IDs
}

async fn slack_event_handler(State(state): State<AppState>, body: Bytes) -> impl IntoResponse {
    let body_str = String::from_utf8_lossy(&body);
    println!(
        "Raw body received at {:?}: {}",
        chrono::Utc::now(),
        body_str
    );

    let payload: SlackEvent = serde_json::from_slice(&body).unwrap_or_else(|e| {
        println!("Deserialization error: {:?}", e);
        SlackEvent {
            event_type: None,
            challenge: None,
            event: None,
            event_id: None,
        }
    });

    if payload.event_type.as_deref() == Some("url_verification") {
        if let Some(challenge) = payload.challenge {
            println!("Received Slack challenge: {}", challenge);
            return Json(serde_json::json!({ "challenge": challenge }));
        }
    }

    if let Some(event) = payload.event {
        println!("Event type: {}", event.msg_type);
        if event.msg_type == "message" {
            if event.bot_id.is_some()
                || event
                    .subtype
                    .as_ref()
                    .map(|s| s == "bot_message")
                    .unwrap_or(false)
            {
                println!("Ignoring bot message: {}", event.text);
                return Json(serde_json::json!({ "status": "ignored" }));
            }

            // Deduplicate events
            let event_id = payload
                .event_id
                .as_ref()
                .unwrap_or(&"unknown".to_string())
                .clone();
            let mut processed = state.processed_events.lock().await;
            if processed.contains(&event_id) {
                println!("Duplicate event ignored: {}", event_id);
                return Json(serde_json::json!({ "status": "ignored" }));
            }
            processed.insert(event_id.clone());
            println!("Processing event ID: {}", event_id);

            println!("Processing message: {}", event.text);
            let slack_token = std::env::var("SLACK_BOT_TOKEN").expect("Missing SLACK_BOT_TOKEN");

            let chat_request = CreateChatCompletionRequestArgs::default()
                .model("gpt-4")
                .messages(vec![
                    ChatCompletionRequestSystemMessageArgs::default()
                        .content("You are a helpful assistant.")
                        .build()
                        .unwrap()
                        .into(),
                    ChatCompletionRequestUserMessageArgs::default()
                        .content(&*event.text)
                        .build()
                        .unwrap()
                        .into(),
                ])
                .build()
                .unwrap();

            let mut stream = match state.client.chat().create_stream(chat_request).await {
                Ok(stream) => stream,
                Err(e) => {
                    eprintln!("Failed to create stream: {:?}", e);
                    let slack_response = SlackResponse {
                        text: format!("Error connecting to OpenAI: {:?}", e),
                        channel: event.channel.clone(),
                    };
                    state
                        .slack_client
                        .post("https://slack.com/api/chat.postMessage")
                        .bearer_auth(&slack_token)
                        .json(&slack_response)
                        .send()
                        .await
                        .ok();
                    return Json(serde_json::json!({ "status": "error" }));
                }
            };

            let mut buffer = String::new();
            const BATCH_SIZE: usize = 1000; // ~1000 chars or sentence end
            const MIN_BATCH_SIZE: usize = 100; // Avoid tiny batches

            while let Some(result) = tokio::time::timeout(Duration::from_secs(300), stream.next())
                .await
                .unwrap_or(None)
            {
                match result {
                    Ok(chat_response) => {
                        for choice in chat_response.choices {
                            if let Some(content) = choice.delta.content {
                                println!("Stream chunk: {}", content);
                                buffer.push_str(&content);

                                // Send if buffer is big enough or ends with a sentence
                                if buffer.len() >= BATCH_SIZE
                                    || (buffer.ends_with('.') && buffer.len() >= MIN_BATCH_SIZE)
                                {
                                    let slack_response = SlackResponse {
                                        text: buffer.clone(),
                                        channel: event.channel.clone(),
                                    };
                                    let res = state
                                        .slack_client
                                        .post("https://slack.com/api/chat.postMessage")
                                        .bearer_auth(&slack_token)
                                        .json(&slack_response)
                                        .send()
                                        .await;
                                    match res {
                                        Ok(_) => println!("Batch sent to Slack successfully."),
                                        Err(e) => {
                                            eprintln!("Failed to send batch to Slack: {:?}", e)
                                        }
                                    }
                                    buffer.clear();
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Stream error: {:?}", e);
                        if !buffer.is_empty() {
                            let slack_response = SlackResponse {
                                text: buffer.clone(),
                                channel: event.channel.clone(),
                            };
                            state
                                .slack_client
                                .post("https://slack.com/api/chat.postMessage")
                                .bearer_auth(&slack_token)
                                .json(&slack_response)
                                .send()
                                .await
                                .ok();
                        }
                        let slack_response = SlackResponse {
                            text: format!("Stream interrupted: {:?}", e),
                            channel: event.channel.clone(),
                        };
                        state
                            .slack_client
                            .post("https://slack.com/api/chat.postMessage")
                            .bearer_auth(&slack_token)
                            .json(&slack_response)
                            .send()
                            .await
                            .ok();
                        break;
                    }
                }
            }

            if !buffer.is_empty() {
                let slack_response = SlackResponse {
                    text: buffer,
                    channel: event.channel.clone(),
                };
                let res = state
                    .slack_client
                    .post("https://slack.com/api/chat.postMessage")
                    .bearer_auth(&slack_token)
                    .json(&slack_response)
                    .send()
                    .await;
                match res {
                    Ok(_) => println!("Final batch sent to Slack successfully."),
                    Err(e) => eprintln!("Failed to send final batch to Slack: {:?}", e),
                }
            }

            return Json(serde_json::json!({ "status": "ok" }));
        }
    }

    println!("Ignoring unknown event: {:?}", payload.event_type);
    Json(serde_json::json!({ "status": "ignored" }))
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    let api_key = std::env::var("OPENAI_API_KEY").expect("Missing OPENAI_API_KEY in .env");
    let config = OpenAIConfig::new().with_api_key(api_key);
    let client = Arc::new(Client::with_config(config));
    let slack_client = HttpClient::new();
    let processed_events = Arc::new(tokio::sync::Mutex::new(HashSet::new()));
    let state = AppState {
        client,
        slack_client,
        processed_events,
    };

    let app = Router::new()
        .route("/slack/events", post(slack_event_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3888));
    println!("Listening on {}", addr);

    axum_server::bind(addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
