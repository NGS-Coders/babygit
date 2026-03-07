mod module_bindings;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use clap::{Args, Parser, Subcommand};
use spacetimedb_sdk::{credentials, DbContext, Table};

use module_bindings::*;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand)]
enum CliCommand {
    Create(CreateArgs),
    List,
}

#[derive(Args)]
struct CreateArgs {
    name: String,

    #[arg(default_value = ".")]
    project_dir: PathBuf,
}

fn read_files_in_dir_inner(
    path: impl AsRef<Path> + Send + 'static,
    level: usize,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let indent = "   ".repeat(level);
        let filename = entry.file_name().into_string().unwrap();

        if entry.path().is_dir() {
            println!("{}└──{}/", indent, filename);
            read_files_in_dir_inner(entry.path(), level + 1)?;
        } else {
            println!("{}└──{}", indent, filename);
        }
    }

    Ok(())
}

fn read_files_in_dir(path: impl AsRef<Path> + Send + 'static) -> anyhow::Result<()> {
    read_files_in_dir_inner(path, 0)
}

fn creds_store() -> credentials::File {
    credentials::File::new("babygit")
}

fn one_shot_lock<T>() -> Arc<OnceLock<T>> {
    Arc::new(OnceLock::new())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // The URI of the SpacetimeDB instance hosting our chat module.
    let host: String = dotenv::var("SPACETIMEDB_HOST")?;

    // The module name we chose when we published our module.
    let db_name: String = dotenv::var("SPACETIMEDB_DB_NAME")?;

    // Connect to the database
    let conn = DbConnection::builder()
        .with_database_name(db_name)
        .with_uri(host)
        .with_token(creds_store().load()?)
        .on_connect(|_, _, token| {
            if let Err(e) = creds_store().save(token) {
                panic!("Failed to save credentials: {:?}", e);
            }
        })
        .on_connect_error(|_ctx, e| {
            eprintln!("Connection error: {:?}", e);
            std::process::exit(1);
        })
        .build()?;

    // Keep connection running in the backgroun
    let conn_handle = conn.run_threaded();

    // Read directory
    // let current_dir = std::env::current_dir()?;
    // read_files_in_dir(current_dir)?;

    match cli.command {
        CliCommand::Create(args) => {
            // Create project entry
            conn.reducers.create_project(args.name)?;

            // TODO: Upload files

            println!("Project created successfully!");
        }
        CliCommand::List => {
            let lock = one_shot_lock();

            let lock_clone = lock.clone();
            conn.subscription_builder()
                .on_applied(move |_| {
                    _ = lock_clone.set(());
                })
                .add_query(|q| q.from.my_projects())
                .subscribe();

            lock.wait();
            let projects = conn.db.my_projects();
            println!("You have {} project(s):", projects.count());
            projects.iter().enumerate().for_each(|(i, p)| {
                println!("{}) {} - {}", i + 1, p.id.to_string(), p.name);
            });
        }
    }

    _ = conn.disconnect();
    _ = conn_handle.join();
    Ok(())
}
