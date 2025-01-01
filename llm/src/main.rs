use serde::{Deserialize, Serialize};

use futures::StreamExt;
use surrealdb::opt::auth::Root;
use surrealdb::RecordId;
use surrealdb::Surreal;

use axum::{
    routing::get,
    Router,
};
use std::net::SocketAddr;
use std::sync::LazyLock;
use surrealdb::engine::remote::ws::{Client, Ws};

use kalosm::{language::*, *};
use std::sync::Arc;
use tokio::sync::Mutex;

use axum::extract::State;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};

static DB: LazyLock<Surreal<Client>> = LazyLock::new(Surreal::init);

#[derive(Parse, Clone)]
pub enum Response {
    Do(String),
    Say(String),
}

#[derive(Clone)]
struct AppState {
    llm: Arc<Mutex<Llama>>,
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
    // TODO: use state
    println!("New WebSocket connection established");

    // Create the parser
    let parser = Arc::new(Response::new_parser());

    let model = {
        // Lock the mutex
        let llama_guard = state.llm.lock().await;
        // Clone it if needed, or directly pass &*llama_guard to the Chat builder
        llama_guard.clone()
    };

    let mut chat = Chat::builder(model)
        .with_constraints(move |_history| parser.clone())
        // .with_system_prompt(
        //     "Respond with JSON in the format { \"type\": \"Say\", \"data\": \"hello\" }",
        // )
        .build();

    // WebSocket message loop
    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Text(text) = msg {
            println!("Received from client: {}", text);
            // Save the received message to the database
            save_message("user".to_string(), text.clone()).await;

            // Add the user message to the chat and process the stream
            let mut response_stream = chat.add_message(text);
            let mut full_response = String::new();

            while let Some(text_chunk) = response_stream.next().await {
                // Append each chunk to the full response
                full_response.push_str(&text_chunk);

                // Optionally send partial updates to the client
                if let Err(e) = socket.send(Message::Text(text_chunk.clone())).await {
                    eprintln!("Error sending chunk: {e}");
                    break;
                }
            }

            save_message("assistant".to_string(), full_response).await;
        }
    }

    println!("WebSocket connection closed");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    DB.connect::<Ws>("localhost:8678").await?;

    DB.signin(Root {
        username: "root",
        password: "root",
    })
    .await?;

    DB.use_ns("test").use_db("test").await?;

    //next-gen-ai thingy
    let llama = Llama::new_chat().await?;
    let state = AppState {
        llm: Arc::new(Mutex::new(llama)),
    };

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
