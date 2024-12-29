use serde::{Deserialize, Serialize};
use std::env;

use surrealdb::opt::auth::Root;
use surrealdb::opt::Resource;
use surrealdb::RecordId;
use surrealdb::Surreal;
use surrealdb::Value;

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use std::net::SocketAddr;
use std::sync::LazyLock;
use surrealdb::engine::remote::ws::{Client, Ws};
use tokio::net::TcpListener;

use kalosm::{language::*, *};
use std::sync::Arc;
use tokio::sync::Mutex;

// use tokio_stream::StreamExt as TokioStreamExt;

// use hyper::server;

use axum::extract::State;
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};

use tokio::sync::mpsc;

static DB: LazyLock<Surreal<Client>> = LazyLock::new(Surreal::init);

#[derive(Clone)]
struct AppState {
    llm: Arc<Mutex<Llama>>,
}

mod error {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::response::Response;
    use axum::Json;
    use thiserror::Error;

    #[derive(Error, Debug)]
    pub enum Error {
        #[error("database error")]
        Db,
    }

    impl IntoResponse for Error {
        fn into_response(self) -> Response {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(self.to_string())).into_response()
        }
    }

    impl From<surrealdb::Error> for Error {
        fn from(error: surrealdb::Error) -> Self {
            eprintln!("{error}");
            Self::Db
        }
    }
}

mod routes {
    use crate::error::Error;
    use crate::DB;

    use axum::{extract::Path, Json};
    use faker_rand::en_us::names::FirstName;
    use serde::{Deserialize, Serialize};
    use surrealdb::{opt::auth::Record, RecordId};

    const PERSON: &str = "person";

    #[derive(Serialize, Deserialize, Clone)]
    pub struct PersonData {
        name: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct Person {
        name: String,
        id: RecordId,
    }

    pub async fn paths() -> &'static str {
        r#"
-----------------------------------------------------------------------------------------------------------------------------------------
        PATH                |           SAMPLE COMMAND
-----------------------------------------------------------------------------------------------------------------------------------------
/session: See session data  |  curl -X GET    -H "Content-Type: application/json"                      http://localhost:8080/session
                            |
/person/{id}:               |
  Create a person           |  curl -X POST   -H "Content-Type: application/json" -d '{"name":"John"}' http://localhost:8080/person/one
  Update a person           |  curl -X PUT    -H "Content-Type: application/json" -d '{"name":"Jane"}' http://localhost:8080/person/one
  Get a person              |  curl -X GET    -H "Content-Type: application/json"                      http://localhost:8080/person/one
  Delete a person           |  curl -X DELETE -H "Content-Type: application/json"                      http://localhost:8080/person/one
                            |
/people: List all people    |  curl -X GET    -H "Content-Type: application/json"                      http://localhost:8080/people

/new_user:  Create a new record user
/new_token: Get instructions for a new token if yours has expired"#
    }

    pub async fn session() -> Result<Json<String>, Error> {
        let res: Option<String> = DB.query("RETURN <string>$session").await?.take(0)?;

        Ok(Json(res.unwrap_or("No session data found!".into())))
    }

    pub async fn create_person(
        id: Path<String>,
        Json(person): Json<PersonData>,
    ) -> Result<Json<Option<Person>>, Error> {
        let person = DB.create((PERSON, &*id)).content(person).await?;
        Ok(Json(person))
    }

    pub async fn read_person(id: Path<String>) -> Result<Json<Option<Person>>, Error> {
        let person = DB.select((PERSON, &*id)).await?;
        Ok(Json(person))
    }

    pub async fn update_person(
        id: Path<String>,
        Json(person): Json<PersonData>,
    ) -> Result<Json<Option<Person>>, Error> {
        let person = DB.update((PERSON, &*id)).content(person).await?;
        Ok(Json(person))
    }

    pub async fn delete_person(id: Path<String>) -> Result<Json<Option<Person>>, Error> {
        let person = DB.delete((PERSON, &*id)).await?;
        Ok(Json(person))
    }

    pub async fn list_people() -> Result<Json<Vec<Person>>, Error> {
        let people = DB.select(PERSON).await?;
        Ok(Json(people))
    }

    #[derive(Serialize, Deserialize)]
    struct Params<'a> {
        name: &'a str,
        pass: &'a str,
    }

    pub async fn make_new_user() -> Result<String, Error> {
        let name = rand::random::<FirstName>().to_string();
        let pass = rand::random::<FirstName>().to_string();
        let jwt = DB
            .signup(Record {
                access: "account",
                namespace: "test",
                database: "test",
                params: Params {
                    name: &name,
                    pass: &pass,
                },
            })
            .await?
            .into_insecure_token();
        Ok(format!("New user created!\n\nName: {name}\nPassword: {pass}\nToken: {jwt}\n\nTo log in, use this command:\n\nsurreal sql --ns test --db test --pretty --token \"{jwt}\""))
    }

    pub async fn get_new_token() -> String {
        let command = r#"curl -X POST -H "Accept: application/json" -d '{"ns":"test","db":"test","ac":"account","user":"your_username","pass":"your_password"}' http://localhost:8000/signin"#;
        format!("Need a new token? Use this command:\n\n{command}\n\nThen log in with surreal sql --ns test --db test --pretty --token YOUR_TOKEN_HERE")
    }
}

// async fn websocket_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
//     ws.on_upgrade(handle_socket)
// }

async fn websocket_handler(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

// async fn handle_socket(mut socket: WebSocket) {
//     println!("New WebSocket connection established");

//     while let Some(Ok(msg)) = socket.recv().await {
//         if let Message::Text(text) = msg {
//             println!("Received: {}", text);

//             // Echo back the same message (replace with Kalosm or SurrealDB logic)
//             if let Err(e) = socket.send(Message::Text(format!("Echo: {}", text))).await {
//                 eprintln!("Error sending message: {}", e);
//                 break;
//             }
//         }
//     }

//     println!("WebSocket connection closed");
// }

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    // TODO: use state
    println!("New WebSocket connection established");

    // Create a Llama instance (will re-load model on each connection)
    let llm = match Llama::new().await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Error initializing Llama: {e}");
            // Optionally send an error message and close
            let _ = socket
                .send(Message::Text("Error initializing Llama".to_string()))
                .await;
            return;
        }
    };

    // Read messages in a loop
    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Text(text) = msg {
            println!("Received from client: {}", text);

            let prompt = text;

            // Stream tokens to the client
            if let Ok(mut stream) = llm.stream_text(&prompt).with_max_length(1000).await {
                while let Some(next_token_result) = stream.next().await {
                    if let Err(e) = socket.send(Message::Text(next_token_result)).await {
                        eprintln!("Error sending token: {e}");
                        break;
                    }
                }
            } else {
                let _ = socket
                    .send(Message::Text("Error generating text".to_string()))
                    .await;
            }
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

    DB.query(
        "
    DEFINE TABLE IF NOT EXISTS person SCHEMALESS
        PERMISSIONS FOR
            CREATE, SELECT WHERE $auth,
            FOR UPDATE, DELETE WHERE created_by = $auth;
    DEFINE FIELD IF NOT EXISTS name ON TABLE person TYPE string;
    DEFINE FIELD IF NOT EXISTS created_by ON TABLE person VALUE $auth READONLY;

    DEFINE INDEX IF NOT EXISTS unique_name ON TABLE user FIELDS name UNIQUE;
    DEFINE ACCESS IF NOT EXISTS account ON DATABASE TYPE RECORD
	SIGNUP ( CREATE user SET name = $name, pass = crypto::argon2::generate($pass) )
	SIGNIN ( SELECT * FROM user WHERE name = $name AND crypto::argon2::compare(pass, $pass) )
	DURATION FOR TOKEN 15m, FOR SESSION 12h
;",
    )
    .await?;

    // let listener = TcpListener::bind("localhost:8080").await?;
    // let router = Router::new()
    //     .route("/", get(routes::paths))
    //     .route("/person/:id", post(routes::create_person))
    //     .route("/person/:id", get(routes::read_person))
    //     .route("/person/:id", put(routes::update_person))
    //     .route("/person/:id", delete(routes::delete_person))
    //     .route("/people", get(routes::list_people))
    //     .route("/session", get(routes::session))
    //     .route("/new_user", get(routes::make_new_user))
    //     .route("/new_token", get(routes::get_new_token));
    // axum::serve(listener, router).await?;

    //next-gen-ai thingy
    let llama = Llama::new().await?;
    let state = AppState {
        llm: Arc::new(Mutex::new(llama)),
    };

    //ws thingy
    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .with_state(state);

    // let app = Router::<()>::new()
    //     .route("/ws", get(websocket_handler))
    //     .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3456));
    println!("Listening on {}", addr);

    axum_server::bind(addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}
