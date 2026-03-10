use spacetimedb::{Identity, ReducerContext, SpacetimeType, Table, Uuid, ViewContext};

#[derive(SpacetimeType)]
enum FileKind {
    File { contents: Vec<u8> },
    Directory,
}

#[spacetimedb::table(accessor = project)]
#[derive(Clone)]
struct Project {
    #[primary_key]
    id: Uuid,

    name: String,

    #[index(btree)]
    author: Identity,

    guests: Vec<Identity>,

    // Need this field because there isn't any way to query every project within reducers otherwise
    #[default(0)]
    #[index(btree)]
    common: u8,
}

impl Project {
    fn new(id: Uuid, name: String, author: Identity) -> Self {
        Self {
            id,
            name,
            author,
            guests: Vec::new(),
            common: 0, // this is crucial to query all projects
        }
    }
}

#[spacetimedb::table(accessor = file)]
struct File {
    #[primary_key]
    id: Uuid,
    path: String,
    kind: FileKind,
    parent_id: Option<Uuid>,
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
fn my_projects(ctx: &ViewContext) -> Vec<Project> {
    log::info!("User {} is querying their projects", ctx.sender().to_hex());

    let all_projects: Vec<Project> = ctx.db.project().common().filter(0u8).collect();
    all_projects
        .iter()
        .filter(|p| p.author == ctx.sender() || p.guests.contains(&ctx.sender()))
        .map(|p| p.clone())
        .collect()
}

#[spacetimedb::reducer]
fn create_project(ctx: &ReducerContext, id: Uuid, name: String) -> anyhow::Result<()> {
    let name = validate_project_name(&name)?;

    ctx.db
        .project()
        .insert(Project::new(id, name.to_owned(), ctx.sender()));
    log::info!(
        "User {} is creating a new project {}",
        ctx.sender().to_hex(),
        &name
    );

    Ok(())
}

#[spacetimedb::reducer]
fn add_guest_to_project(
    ctx: &ReducerContext,
    project_id: Uuid,
    guest_id: Identity,
) -> anyhow::Result<()> {
    // Fetch project
    let mut project = ctx
        .db
        .project()
        .id()
        .find(project_id)
        .ok_or(anyhow::anyhow!("Project not found"))?;

    // Check ownership
    anyhow::ensure!(project.author == ctx.sender(), "Project not found");

    // Check if guest is already part of this project
    anyhow::ensure!(
        project.author != guest_id && !project.guests.contains(&guest_id),
        "This person is already part of this project"
    );

    // Add guest to project
    project.guests.push(guest_id);
    ctx.db.project().id().update(project);

    Ok(())
}

#[spacetimedb::reducer]
fn add_file_to_project(
    ctx: &ReducerContext,
    id: Uuid,
    project_id: Uuid,
    path: String,
    kind: FileKind,
    parent_id: Option<Uuid>,
) -> anyhow::Result<()> {
    let path = path; // TODO: validate

    ctx.db.file().insert(File {
        id,
        path,
        kind,
        parent_id,
        project_id,
    });
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
