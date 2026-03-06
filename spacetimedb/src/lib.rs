use spacetimedb::{Identity, Query, ReducerContext, SpacetimeType, Table, Uuid, ViewContext};

#[derive(SpacetimeType)]
enum FileKind {
    File { contents: Vec<u8> },
    Directory,
}

#[spacetimedb::table(accessor = project)]
struct Project {
    #[primary_key]
    id: Uuid,
    name: String,
    #[index(btree)]
    author: Identity,
    guests: Vec<Identity>,
}

#[spacetimedb::table(accessor = file)]
struct File {
    #[primary_key]
    id: Uuid,
    path: String,
    kind: FileKind,
    parent_id: Uuid,
    project_id: Uuid,
}

#[spacetimedb::reducer(init)]
fn init(_ctx: &ReducerContext) {
    // Called when the module is initially published
}

#[spacetimedb::reducer(client_connected)]
fn identity_connected(ctx: &ReducerContext) {
    // Called everytime a new client connects
    log::debug!("Client connected: {:?}", ctx.identity());
}

#[spacetimedb::reducer(client_disconnected)]
fn identity_disconnected(ctx: &ReducerContext) {
    // Called everytime a client disconnects
    log::debug!("Client disconnected: {:?}", ctx.identity());
}

#[spacetimedb::view(accessor = my_projects, public)]
fn my_projects(ctx: &ViewContext) -> impl Query<Project> {
    // TODO: Also show projects where sender is a guest
    ctx.from.project().r#where(|p| p.author.eq(ctx.sender()))
}

#[spacetimedb::reducer]
fn create_project(ctx: &ReducerContext, name: String) -> anyhow::Result<()> {
    let name = validate_project_name(&name)?;
    let id = ctx.new_uuid_v7()?;

    ctx.db.project().insert(Project {
        id,
        name: name.to_string(),
        author: ctx.identity(),
        guests: Vec::new(),
    });
    log::info!(
        "User {} is creating a new project {}",
        ctx.identity().to_hex(),
        &name
    );

    Ok(())
}

fn validate_project_name(text: &str) -> anyhow::Result<&str> {
    let trimmed = text.trim();
    anyhow::ensure!(!trimmed.is_empty(), "Project name cannot be empty");
    anyhow::ensure!(
        trimmed.len() <= 64,
        "Project name cannot exceed 64 characters"
    );
    anyhow::ensure!(
        trimmed.is_ascii(),
        "Project name cannot contain non-ASCII characters"
    );

    Ok(trimmed)
}
