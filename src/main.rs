mod module_bindings;

use std::{future::Future, path::Path, pin::Pin};

// use module_bindings::*;
use tokio::fs;

fn read_files_in_dir_inner(
    path: impl AsRef<Path> + Send + 'static,
    level: usize,
) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> {
    Box::pin(async move {
        let mut dir = fs::read_dir(path).await?;

        while let Some(entry) = dir.next_entry().await? {
            let indent = "   ".repeat(level);
            let filename = entry.file_name().into_string().unwrap();

            if entry.path().is_dir() {
                println!("{}└──{}/", indent, filename);
                read_files_in_dir_inner(entry.path(), level + 1).await?;
            } else {
                println!("{}└──{}", indent, filename);
            }
        }

        Ok(())
    })
}

async fn read_files_in_dir(path: impl AsRef<Path> + Send + 'static) -> anyhow::Result<()> {
    read_files_in_dir_inner(path, 0).await
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    /*
    // The URI of the SpacetimeDB instance hosting our chat module.
    let host: String = dotenv::var("SPACETIMEDB_HOST")?;

    // The module name we chose when we published our module.
    let db_name: String = dotenv::var("SPACETIMEDB_DB_NAME")?;

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
        .build()?;

    // Keep connection running in the backgroun
    tokio::spawn(async move {
        conn.run_async().await.unwrap();
    });
    */

    // Read directory
    let current_dir = std::env::current_dir().unwrap();
    read_files_in_dir(current_dir).await?;

    Ok(())
}
