mod module_bindings;
use module_bindings::*;

#[tokio::main]
async fn main() {
    // The URI of the SpacetimeDB instance hosting our chat module.
    let host: String =
        dotenv::var("SPACETIMEDB_HOST").unwrap_or("http://localhost:3000".to_string());

    // The module name we chose when we published our module.
    let db_name: String = dotenv::var("SPACETIMEDB_DB_NAME").unwrap_or("my-db".to_string());

    // Connect to the database
    let conn = DbConnection::builder()
        .with_database_name(db_name)
        .with_uri(host)
        .on_connect(|_, _, _| {
            println!("Connected to SpacetimeDB");
        })
        .on_connect_error(|_ctx, e| {
            eprintln!("Connection error: {:?}", e);
            std::process::exit(1);
        })
        .build()
        .expect("Failed to connect");

    conn.run_async().await.unwrap();
}
