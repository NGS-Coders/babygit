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

fn creds_store() -> credentials::File {
    credentials::File::new("babygit")
}

fn one_shot_lock<T>() -> Arc<OnceLock<T>> {
    Arc::new(OnceLock::new())
}

fn upload_project_dir(
    conn: &DbConnection,
    uuid_ctx: &uuid::ContextV7,
    project_id: spacetimedb_sdk::Uuid,
    dir_path: impl AsRef<Path>,
    dir_id: Option<spacetimedb_sdk::Uuid>,
) -> anyhow::Result<usize> {
    let mut total_file_count = 0usize;

    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();
        let path_str = path
            .to_str()
            .ok_or(anyhow::anyhow!("Invalid path"))?
            .to_owned();

        let file_id = {
            let ts = uuid::Timestamp::now(uuid_ctx);
            let uuid = uuid::Uuid::new_v7(ts);
            spacetimedb_sdk::Uuid::from_u128(uuid.as_u128())
        };

        let (kind, file_count) = if entry.path().is_dir() {
            let scanned_files =
                upload_project_dir(conn, uuid_ctx, project_id, &path, Some(file_id))?;
            (FileKind::Directory, scanned_files)
        } else {
            let contents = fs::read(&path)?;
            (FileKind::File(contents), 1usize)
        };

        conn.reducers
            .add_file_to_project(file_id, project_id, path_str, kind, dir_id)?;
        println!("Uploaded {:?}", path);
        total_file_count += file_count;
    }

    Ok(total_file_count)
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

    match cli.command {
        CliCommand::Create(args) => {
            let uuid_ctx = uuid::ContextV7::new();

            // Create project entry
            let project_id = {
                let ts = uuid::Timestamp::now(&uuid_ctx);
                let uuid = uuid::Uuid::new_v7(ts);
                spacetimedb_sdk::Uuid::from_u128(uuid.as_u128())
            };
            conn.reducers.create_project(project_id, args.name)?;

            // Upload project files
            let files_uploaded =
                upload_project_dir(&conn, &uuid_ctx, project_id, args.project_dir, None)?;
            println!(
                "Project created successfully! Uploaded {} file(s)",
                files_uploaded
            );
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
