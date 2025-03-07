use async_openai::Client;
use async_openai::config::OpenAIConfig;
use async_openai::types::{
    ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use axum::{Json, Router, routing::get, routing::post};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use axum::extract::State;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};
use dotenvy::dotenv;
use reqwest::Client as HttpClient;
use std::net::SocketAddr;
use std::sync::Arc;

#[derive(Deserialize)]
struct SlackEvent {
    #[serde(rename = "type")]
    event_type: String,
    challenge: Option<String>, // For Slack URL verification
    event: Option<SlackMessageEvent>,
}

#[derive(Deserialize)]
struct SlackMessageEvent {
    text: String,
    user: String,
    channel: String,
    #[serde(rename = "type")]
    msg_type: String,
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

async fn slack_event_handler(
    State(state): State<AppState>, // Correctly extract State
    Json(payload): Json<SlackEvent>,
) -> impl IntoResponse {
    if let Some(challenge) = payload.challenge {
        return Json(serde_json::json!({ "challenge": challenge }));
    }

    if let Some(event) = payload.event {
        if event.msg_type == "message" {
            println!("Received message: {}", event.text);

            let response = call_openai(&state.client, event.text).await;

            let slack_client = HttpClient::new();
            let slack_token = std::env::var("SLACK_BOT_TOKEN").expect("Missing SLACK_BOT_TOKEN");

            let slack_response = SlackResponse {
                text: response,
                channel: event.channel,
            };

            // Send response back to Slack
            let _ = slack_client
                .post("https://slack.com/api/chat.postMessage")
                .bearer_auth(slack_token)
                .json(&slack_response)
                .send()
                .await;

            return Json(serde_json::json!({ "status": "ok" }));
        }
    }

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

    // Create a streamed response
    let mut stream = client
        .chat()
        .create_stream(chat_request)
        .await
        .expect("Failed to create stream");

    let mut full_response = String::new();

    while let Some(response) = stream.next().await {
        match response {
            Ok(chat_response) => {
                for choice in chat_response.choices {
                    if let Some(content) = choice.delta.content {
                        full_response.push_str(&content);
                    }
                }
            }
            Err(e) => {
                eprintln!("Stream error: {:?}", e);
                break;
            }
        }
    }

    if full_response.is_empty() {
        "Oops, something went wrong.".to_string()
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
