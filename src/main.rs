use std::env;
use serde::{Deserialize, Serialize};
use surrealdb::engine::remote::ws::Ws;
use surrealdb::opt::auth::Root;
use surrealdb::opt::Resource;
use surrealdb::RecordId;
use surrealdb::Surreal;
use surrealdb::Value;

#[derive(Debug, Serialize)]
struct Name<'a> {
    first: &'a str,
    last: &'a str,
}

#[derive(Debug, Serialize)]
struct Person<'a> {
    title: &'a str,
    name: Name<'a>,
    marketing: bool,
}

#[derive(Debug, Serialize)]
struct Responsibility {
    marketing: bool,
}

#[derive(Debug, Deserialize)]
struct Record {
    id: RecordId,
}

#[tokio::main]
async fn main() -> surrealdb::Result<()> {
    let host: String = match env::args().nth(1) {
        Some(host) => host,
        None => "".to_string(),
    };

    let db = Surreal::new::<Ws>("127.0.0.1:8678").await?;

    db.signin(Root {
        username: "root",
        password: "root",
    })
    .await?;

    db.use_ns("test").use_db("test").await?;

    // Create a new person with a random id
    let created: Option<Record> = db
        .create("person")
        .content(Person {
            title: "Founder & CEO",
            name: Name {
                first: "Tobie",
                last: "Morgan Hitchcock",
            },
            marketing: true,
        })
        .await?;
    dbg!(created);

    // Update a person record with a specific id
    // We don't care about the response in this case
    // so we are just going to use `Resource::from`
    // to let the compiler return `surrealdb::Value`
    db.update(Resource::from(("person", "jaime")))
        .merge(Responsibility { marketing: true })
        .await?;

    // Select all people records
    let people: Vec<Record> = db.select("person").await?;
    dbg!(people);

    // Perform a custom advanced query
    let mut groups = db
        .query("SELECT marketing, count() FROM type::table($table) GROUP BY marketing")
        .bind(("table", "person"))
        .await?;
    // Use .take() to transform the first query result into
    // anything that can be deserialized, in this case
    // a Value
    dbg!(groups.take::<Value>(0).unwrap());

    Ok(())
}
