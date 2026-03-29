mod file_tree;
mod module_bindings;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock, RwLock},
};

use clap::{Args, Parser, Subcommand};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use spacetimedb_sdk::{credentials, DbContext, Identity, Table};

use file_tree::FileTree;
use module_bindings::*;

static IDENTITY_LOCK: OnceLock<Identity> = OnceLock::new();

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand)]
enum CliCommand {
    Me,
    Create(CreateArgs),
    AddGuest(AddGuestArgs),
    List,
    Work(WorkArgs),
}

#[derive(Args)]
struct CreateArgs {
    name: String,

    #[arg(short, long, default_value = ".")]
    project_dir: PathBuf,
}

#[derive(Args)]
struct AddGuestArgs {
    guest_id: Identity,

    #[arg(short, long)]
    project_id: uuid::Uuid,
}

#[derive(Args)]
struct WorkArgs {
    project_id: uuid::Uuid,

    #[arg(short, long, default_value = ".")]
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
    project_path: &Path,
    dir_path: impl AsRef<Path>,
    dir_id: Option<spacetimedb_sdk::Uuid>,
) -> anyhow::Result<usize> {
    let mut total_file_count = 0usize;

    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();

        let path_str = {
            let path_absolute = path.canonicalize()?;
            path_absolute
                .strip_prefix(project_path)?
                .to_str()
                .ok_or(anyhow::anyhow!("Invalid path"))?
                .to_owned()
        };

        let file_id = {
            let ts = uuid::Timestamp::now(uuid_ctx);
            let uuid = uuid::Uuid::new_v7(ts);
            spacetimedb_sdk::Uuid::from_u128(uuid.as_u128())
        };

        let (kind, file_count) = if entry.path().is_dir() {
            let scanned_files = upload_project_dir(
                conn,
                uuid_ctx,
                project_id,
                &project_path,
                &path,
                Some(file_id),
            )?;
            (FileKind::Directory, scanned_files)
        } else {
            let contents = fs::read(&path)?;
            let hash = crc32fast::hash(&contents);

            (
                FileKind::File(FileContents {
                    hash,
                    data: contents,
                }),
                1usize,
            )
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
        .on_connect(|_, identity, token| {
            if let Err(e) = creds_store().save(token) {
                panic!("Failed to save credentials: {:?}", e);
            }
            _ = IDENTITY_LOCK.set(identity);
        })
        .on_connect_error(|_ctx, e| {
            eprintln!("Connection error: {:?}", e);
            std::process::exit(1);
        })
        .build()?;

    // Keep connection running in the backgroun
    let conn_handle = conn.run_threaded();

    match cli.command {
        CliCommand::Me => {
            let identity = IDENTITY_LOCK.wait();
            println!("User ID: {}", identity);
        }

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
            let project_dir = args.project_dir.canonicalize().unwrap();
            let files_uploaded = upload_project_dir(
                &conn,
                &uuid_ctx,
                project_id,
                &project_dir,
                &project_dir,
                None,
            )?;
            println!(
                "Project created successfully! Uploaded {} file(s)",
                files_uploaded
            );
        }

        CliCommand::AddGuest(args) => {
            conn.reducers.add_guest_to_project_then(
                spacetimedb_sdk::Uuid::from_u128(args.project_id.as_u128()),
                args.guest_id,
                |_, result| match result {
                    Ok(res) => match res {
                        Ok(()) => println!("Added guest to project!"),
                        Err(e) => eprintln!("Error: {}", e),
                    },
                    Err(e) => {
                        eprintln!("Error: {}", e);
                    }
                },
            )?;
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

        CliCommand::Work(args) => {
            let project_dir = args.project_dir.canonicalize()?;

            let tree = Arc::new(RwLock::new(FileTree::new(
                args.project_id.clone(),
                project_dir.clone(),
            )));

            // Callback needs to be registered before creating subscriptions
            let tree_clone = tree.clone();
            let project_dir_clone = project_dir.clone();
            conn.db.file().on_insert(move |_, file| {
                let tree = tree_clone.read().unwrap();
                if !tree.is_ready() {
                    return;
                }

                let file_path = project_dir_clone.join(&file.path);

                // TODO: Update file tree

                match file.kind {
                    FileKind::File(ref contents) => {
                        println!("File received:\t{:?}", file_path.display());

                        if let Some(parent) = file_path.parent() {
                            fs::create_dir_all(parent).unwrap();
                        }
                        fs::write(file_path, &contents.data).unwrap();
                    }
                    FileKind::Directory => {
                        println!("Dir received:\t{:?}", file_path.display());
                        fs::create_dir_all(file_path).unwrap();
                    }
                }
            });

            let tree_clone = tree.clone();
            conn.subscription_builder()
                .on_applied(move |ctx| {
                    let mut tree = tree_clone.write().unwrap();

                    // Queue initial files for tree construction
                    ctx.db.file().iter().for_each(|file| {
                        let file_uuid = uuid::Uuid::from_u128(file.id.as_u128());

                        match file.kind {
                            FileKind::File(contents) => {
                                tree.queue_file(file_uuid, Some(contents.hash), &file.path)
                            }
                            FileKind::Directory => tree.queue_file(file_uuid, None, &file.path),
                        }
                        .unwrap()
                    });

                    // Initial file rows have been received, begin building file tree
                    tree.build().unwrap();
                    println!("Tree built :)")
                })
                .add_query(|q| {
                    q.from
                        .file()
                        .filter(|f| f.project_id.eq(args.project_id.as_u128()))
                })
                .subscribe();

            let project_dir_clone = project_dir.clone();
            let tree_clone = tree.clone();
            let mut watcher = RecommendedWatcher::new(
                move |res: Result<notify::Event, notify::Error>| match res {
                    Ok(event) => {
                        let path = event.paths.first().unwrap();

                        match event.kind {
                            EventKind::Create(_) => {
                                println!("File created:\t{:?}", path.display());
                                // TODO: sync new file to db
                            }
                            EventKind::Modify(_) => {
                                // Skip if a directory was modified
                                if path.is_dir() {
                                    return;
                                }

                                let stripped_path = path.strip_prefix(&project_dir_clone).unwrap();
                                let file_contents = fs::read(path);
                                if file_contents.is_err() {
                                    return;
                                }
                                let file_hash = crc32fast::hash(&file_contents.unwrap());

                                let file_node = {
                                    let tree = tree_clone.read().unwrap();
                                    tree.get_file(stripped_path).unwrap()
                                };
                                if file_node.is_none() {
                                    eprintln!("Not in tree:\t{}", path.display());
                                    return;
                                }
                                let file_node = file_node.unwrap();
                                let mut file_node = file_node.lock().unwrap();
                                let stored_hash = file_node
                                    .hash
                                    .expect("Non-directory files MUST have a hash");

                                // Ignore if file contents weren't changed
                                if stored_hash == file_hash {
                                    // println!("Skipping file change:\t{}", path.display());
                                    return;
                                }

                                // Update file hash in tree
                                file_node.hash = Some(file_hash);

                                println!(
                                    "File changed:\t{:?}\n\t\t{} -> {:?}",
                                    path.display(),
                                    file_hash,
                                    stored_hash
                                );
                                // TODO: sync file contents to db
                            }
                            EventKind::Remove(_) => {
                                println!("File deleted:\t{:?}", path.display());
                                // TODO: delete file from db
                            }
                            _ => {}
                        }
                    }
                    Err(err) => eprintln!("Watch error: {:?}", err),
                },
                Config::default(),
            )?;
            watcher.watch(&project_dir, RecursiveMode::Recursive)?;

            loop {
                // Keep connection alive
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    }

    _ = conn.disconnect();
    _ = conn_handle.join();
    Ok(())
}
