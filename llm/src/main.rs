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

/// A small helper to remove (most) awkward spaces from a chunk of text
/// before appending it to `full_response`.
fn normalize_chunk(chunk: &str, current_buffer: &str) -> String {
    // 1) Trim leading/trailing whitespace
    let trimmed = chunk.trim();
    if trimmed.is_empty() {
        return "".to_string();
    }

    // 2) Optionally insert a space if the last character in `current_buffer`
    //    is alphanumeric (or certain punctuation) and the first character of
    //    trimmed is alphanumeric (or if it doesn't start with punctuation).
    let last_char_opt = current_buffer.chars().last();
    let first_char_opt = trimmed.chars().next();

    let mut final_chunk = String::new();

    if let (Some(last_char), Some(first_char)) = (last_char_opt, first_char_opt) {
        // If the last char in our buffer is a letter/digit, and the chunk
        // starts with a letter/digit, we add a space.
        if last_char.is_alphanumeric() && first_char.is_alphanumeric() {
            final_chunk.push(' ');
        }
        // Or if the last char is a letter/digit and the chunk starts with an opening quote
        // or something that might need spacing, etc. (You can expand rules here.)
    }

    final_chunk.push_str(trimmed);

    // 3) Light replacements that remove spaces before punctuation:
    //    " ," -> ","  |  " ." -> "."  |  " !" -> "!"  |  " ?" -> "?"
    //    This is simplistic; adapt or remove as needed.
    let final_chunk = final_chunk
        .replace(" ,", ",")
        .replace(" .", ".")
        .replace(" !", "!")
        .replace(" ?", "?")
        .replace(" ;", ";")
        .replace(" :", ":");

    final_chunk
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
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
        // If you had custom constraints or system prompts, you could do that here
        .with_system_prompt("Respond briefly with a snarky sentence or two.")
        .build();

    // We'll keep appending to `full_response` as the partial text arrives
    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Text(text) = msg {
            println!("Received from client: {}", text);

            // Save the incoming (user) message
            save_message("user".to_string(), text.clone()).await;

            // Add user message to the chat and process the stream
            let mut response_stream = chat.add_message(text);
            let mut full_response = String::new();

            while let Some(text_chunk) = response_stream.next().await {
                // Clean up chunk
                let normalized = normalize_chunk(&text_chunk, &full_response);

                // Only append/send if there's something left after trimming
                if !normalized.is_empty() {
                    full_response.push_str(&normalized);

                    // Send partial update to the client
                    if let Err(e) = socket.send(Message::Text(normalized)).await {
                        eprintln!("Error sending chunk: {e}");
                        break;
                    }
                }
            }

            // Save the entire assistant response once it's complete
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

    // Next-gen LLM
    let llama = Llama::new_chat().await?;
    let state = AppState {
        llm: Arc::new(Mutex::new(llama)),
    };

    // Build the Axum app with WebSocket route
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
