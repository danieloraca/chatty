use async_openai::config::OpenAIConfig;
use async_openai::types::{
    ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use async_openai::Client;

use dotenvy::dotenv;
use serde::{Deserialize, Serialize};

use surrealdb::opt::auth::Root;
use surrealdb::RecordId;
use surrealdb::Surreal;

use axum::extract::State;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};
use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::LazyLock;
use surrealdb::engine::remote::ws::Client as SurrealClient;
use surrealdb::engine::remote::ws::Ws;

static DB: LazyLock<Surreal<SurrealClient>> = LazyLock::new(Surreal::init);

#[derive(Clone)]
pub enum Response {
    Do(String),
    Say(String),
}

#[derive(Clone)]
struct AppState {
    client: Arc<Client<OpenAIConfig>>,
}

#[derive(Serialize, Deserialize)]
struct MessageRecord {
    content: String,
    sender: String,
    timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    // You can expand these fields or rename them
    pub role: String,    // e.g. "user" or "assistant"
    pub content: String, // the message text
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub id: Option<RecordId>, // SurrealDB auto-generated ID
}

async fn websocket_handler(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn save_message(role: String, content: String) {
    let user_msg = ChatMessage {
        role,
        content,
        timestamp: chrono::Utc::now(),
        id: None,
    };

    let create_user_message: Result<std::option::Option<ChatMessage>, surrealdb::Error> =
        DB.create("messages").content(user_msg).await;

    match create_user_message {
        Ok(Some(record)) => {
            println!("Inserted record with ID {:?}", record.id);
        }
        Ok(None) => {
            println!("No record returned (check your Surreal schema).");
        }
        Err(e) => {
            eprintln!("Error saving user message: {}", e);
        }
    }
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    println!("New WebSocket connection established");

    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Text(text) = msg {
            println!("Received from client: {}", text);

            save_message("user".to_string(), text.clone()).await;

            // Call OpenAI API
            let response = call_openai(&state.client, text.clone()).await;

            // Send response to client
            if let Err(e) = socket.send(Message::Text(response.clone())).await {
                eprintln!("Error sending message: {e}");
            }

            save_message("assistant".to_string(), response).await;
        }
    }

    println!("WebSocket connection closed");
}

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

    match client.chat().create(chat_request).await {
        Ok(response) => response
            .choices
            .first()
            .map(|c| c.message.content.clone().unwrap_or_default())
            .unwrap_or_default(),
        Err(e) => {
            eprintln!("Error calling OpenAI API: {e}");
            "Oops, something went wrong.".to_string()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    DB.connect::<Ws>("localhost:8678").await?;

    DB.signin(Root {
        username: "root",
        password: "root",
    })
    .await?;

    DB.use_ns("test").use_db("test").await?;

    //next-gen-ai thingy
    let api_key = std::env::var("OPENAI_API_KEY").expect("Missing OPENAI_API_KEY in .env");
    let config = OpenAIConfig::new().with_api_key(api_key);
    let client = Arc::new(Client::with_config(config));

    let state = AppState { client };

    //ws thingy
    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3456));
    println!("Listening on {}", addr);

    axum_server::bind(addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}
