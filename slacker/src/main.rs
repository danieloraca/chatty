use async_openai::config::OpenAIConfig;
use async_openai::types::{
    ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use async_openai::Client;
use axum::{routing::get, routing::post, Json, Router};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use axum::body::Bytes;
use axum::extract::State;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};
use dotenvy::dotenv;
use reqwest::Client as HttpClient;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

#[derive(Deserialize, Serialize, Debug)]
struct SlackEvent {
    #[serde(rename = "type")]
    event_type: Option<String>,
    challenge: Option<String>, // For Slack URL verification
    event: Option<SlackMessageEvent>,
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
pub enum Response {
    Do(String),
    Say(String),
}

#[derive(Clone)]
struct AppState {
    client: Arc<Client<OpenAIConfig>>,
}

// async fn websocket_handler(
//     State(state): State<AppState>,
//     ws: WebSocketUpgrade,
// ) -> impl IntoResponse {
//     ws.on_upgrade(move |socket| handle_socket(socket, state))
// }

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
        }
    });

    if payload.event_type.as_deref() == Some("url_verification") {
        if let Some(challenge) = payload.challenge {
            println!("Received Slack challenge: {}", challenge);
            return Json(serde_json::json!({ "challenge": challenge }));
        } else {
            println!("URL verification missing challenge field");
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

            println!("Processing message: {}", event.text);
            let response = call_openai(&state.client, event.text).await;
            println!("OpenAI response: {:?}", response); // Log the response
            let slack_client = HttpClient::new();
            let slack_token = std::env::var("SLACK_BOT_TOKEN").expect("Missing SLACK_BOT_TOKEN");

            // Split response into 4000-char chunks
            let mut chunks = Vec::new();
            let mut remaining = response.as_str();
            while !remaining.is_empty() {
                let (chunk, rest) = if remaining.len() > 4000 {
                    let split_at = remaining[..4000].rfind(' ').unwrap_or(4000);
                    (&remaining[..split_at], &remaining[split_at..])
                } else {
                    (remaining, "")
                };
                chunks.push(chunk.to_string());
                remaining = rest.trim_start();
            }

            // Send each chunk
            for (i, chunk) in chunks.iter().enumerate() {
                let slack_response = SlackResponse {
                    text: if chunks.len() > 1 {
                        format!("Part {}/{}: {}", i + 1, chunks.len(), chunk)
                    } else {
                        chunk.clone()
                    },
                    channel: event.channel.clone(),
                };

                let res = slack_client
                    .post("https://slack.com/api/chat.postMessage")
                    .bearer_auth(&slack_token)
                    .json(&slack_response)
                    .send()
                    .await;

                match res {
                    Ok(_) => println!("Message part {} sent to Slack successfully.", i + 1),
                    Err(e) => eprintln!("Failed to send message part {} to Slack: {:?}", i + 1, e),
                }
            }

            return Json(serde_json::json!({ "status": "ok" }));
        }
    }

    println!("Ignoring unknown event: {:?}", payload.event_type);
    Json(serde_json::json!({ "status": "ignored" }))
}

// async fn handle_socket(mut socket: WebSocket, state: AppState) {
//     println!("New WebSocket connection established");

//     while let Some(Ok(msg)) = socket.recv().await {
//         if let Message::Text(text) = msg {
//             println!("Received from client: {}", text);

//             // Call OpenAI API
//             let response = call_openai(&state.client, text.clone()).await;

//             // Send response to client
//             if let Err(e) = socket.send(Message::Text(response.clone())).await {
//                 eprintln!("Error sending message: {e}");
//             }
//         }
//     }

//     println!("WebSocket connection closed");
// }

async fn call_openai(client: &Client<OpenAIConfig>, user_input: String) -> String {
    let chat_request = CreateChatCompletionRequestArgs::default()
        .model("gpt-4")
        .messages(vec![
            ChatCompletionRequestSystemMessageArgs::default()
                .content("You are a helpful assistant.")
                .build()
                .unwrap()
                .into(),
            ChatCompletionRequestUserMessageArgs::default()
                .content(user_input)
                .build()
                .unwrap()
                .into(),
        ])
        .build()
        .unwrap();

    let mut stream = match client.chat().create_stream(chat_request).await {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("Failed to create stream: {:?}", e);
            return format!("Error connecting to OpenAI: {:?}", e);
        }
    };

    let mut full_response = String::new();
    while let Some(result) = tokio::time::timeout(Duration::from_secs(30), stream.next())
        .await
        .unwrap_or(None)
    {
        match result {
            Ok(chat_response) => {
                for choice in chat_response.choices {
                    if let Some(content) = choice.delta.content {
                        full_response.push_str(&content);
                        println!("Stream chunk: {}", content);
                    }
                }
            }
            Err(e) => {
                eprintln!("Stream error: {:?}", e);
                return format!("Error streaming from OpenAI: {:?}", e);
            }
        }
    }

    if full_response.is_empty() {
        "No response generated.".to_string()
    } else {
        full_response
    }
}

#[tokio::main]
async fn main() {
    dotenv().ok();

    let api_key = std::env::var("OPENAI_API_KEY").expect("Missing OPENAI_API_KEY in .env");
    let config = OpenAIConfig::new().with_api_key(api_key);
    let client = Arc::new(Client::with_config(config));

    let state = AppState { client };

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
