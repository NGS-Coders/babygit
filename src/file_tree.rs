use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::Mutex,
    sync::{Arc, Weak},
};

use uuid::Uuid;

/// Keys are paths _relative_ to the parent
type Children = HashMap<PathBuf, Arc<Mutex<FileTreeNode>>>;

#[derive(Clone)]
pub struct FileTreeNode {
    pub id: Uuid,
    pub name: String,
    pub path: PathBuf,
    pub parent: Option<Weak<FileTreeNode>>,
    pub children: Children,
    pub hash: Option<u32>,
}

pub struct FileTree {
    pub project_id: Uuid,
    pub root_dir: PathBuf,
    pub children: Children,
    file_queue: Vec<(Uuid, Option<u32>, PathBuf)>,
    constructed: bool,
}

impl FileTree {
    pub fn new(project_id: Uuid, root_dir: impl AsRef<Path>) -> Self {
        Self {
            project_id,
            root_dir: root_dir.as_ref().to_owned(),
            children: HashMap::new(),
            file_queue: vec![],
            constructed: false,
        }
    }

    pub fn queue_file(
        &mut self,
        file_id: Uuid,
        hash: Option<u32>,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        if self.constructed {
            anyhow::bail!("Cannot queue files after the file tree has been built");
        }

        self.file_queue
            .push((file_id, hash, path.as_ref().to_owned()));
        Ok(())
    }

    pub fn build(&mut self) -> anyhow::Result<()> {
        // Sort file queue by UUIDv7 IDs. This ensures that files aren't added to the tree before
        // their parent directories.
        self.file_queue.sort_by_key(|(id, _, _)| *id);

        let queue = self.file_queue.clone();
        for (file_id, hash, file_path) in queue {
            self.add(file_id, hash, &file_path)?;
        }

        self.constructed = true;
        Ok(())
    }

    pub fn is_ready(&self) -> bool {
        self.constructed
    }

    pub fn get_file(
        &self,
        file_path: impl AsRef<Path>,
    ) -> anyhow::Result<Option<Arc<Mutex<FileTreeNode>>>> {
        let file_path = file_path.as_ref();

        // Ensure the given path is a relative path from the project root
        let file_path = if file_path.is_absolute() {
            file_path.strip_prefix(&self.root_dir)?
        } else {
            file_path
        };

        let file_parent = file_path.parent().unwrap();
        let first_comp = file_parent.components().next();

        if let Some(first_comp) = first_comp {
            // File is likely under a subdirectory
            if let Component::Normal(first_comp) = first_comp {
                let comp_path = Path::new(first_comp);

                if let Some(parent_node) = self.children.get(comp_path).cloned() {
                    self.get_file_subdirectory(file_path, parent_node)
                } else {
                    Ok(None)
                }
            } else {
                anyhow::bail!("File's path component was not normal")
            }
        } else {
            // Check if file is directly under root directory
            let node = self.children.get(file_path).cloned();
            Ok(node)
        }
    }

    fn get_file_subdirectory(
        &self,
        file_path: impl AsRef<Path>,
        parent_node: Arc<Mutex<FileTreeNode>>,
    ) -> anyhow::Result<Option<Arc<Mutex<FileTreeNode>>>> {
        let file_path = file_path.as_ref(); // Guaranteed to be relative to project root
        let file_parent = file_path.parent().unwrap();
        let parent_node = parent_node.lock().unwrap();

        let first_comp = file_parent
            .strip_prefix(&parent_node.path)?
            .components()
            .next();

        if let Some(first_comp) = first_comp {
            // File is likely under a subdirectory
            if let Component::Normal(first_comp) = first_comp {
                let comp_path = Path::new(first_comp);

                if let Some(parent_node) = parent_node.children.get(comp_path).cloned() {
                    self.get_file_subdirectory(file_path, parent_node)
                } else {
                    Ok(None)
                }
            } else {
                anyhow::bail!("File's path component was not normal")
            }
        } else {
            // File is likely directly under parent node
            let file_name = Path::new(file_path.file_name().unwrap());
            let node = parent_node.children.get(file_name).cloned();
            Ok(node)
        }
    }

    pub fn add(
        &mut self,
        file_id: Uuid,
        hash: Option<u32>,
        file_path: impl AsRef<Path>,
    ) -> anyhow::Result<Arc<Mutex<FileTreeNode>>> {
        let file_path = file_path.as_ref();

        // Ensure the given path is a relative path from the project root
        let file_path = if file_path.is_absolute() {
            file_path.strip_prefix(&self.root_dir)?
        } else {
            file_path
        };

        // Check if this file is directly under the root directory
        let file_parent = file_path.parent().unwrap();
        if file_parent == Path::new("") {
            if self.children.contains_key(file_path) {
                anyhow::bail!("This path already exists in the file tree");
            }

            let new_node = FileTreeNode {
                id: file_id,
                name: file_path.file_name().unwrap().to_str().unwrap().to_owned(),
                path: file_path.to_owned(),
                parent: None,
                children: HashMap::new(),
                hash,
            };
            let value = Arc::new(Mutex::new(new_node));
            self.children.insert(file_path.to_owned(), value.clone());
            return Ok(value);
        }

        // File is under a subdirectory
        let components = file_parent.components().collect::<Vec<_>>();
        let first_comp = components.first().unwrap();
        if let Component::Normal(first_comp) = first_comp {
            let comp_path = Path::new(first_comp);

            if let Some(node) = self.children.get_mut(comp_path).cloned() {
                self.add_subdirectory(file_id, hash, file_path, node)
            } else {
                anyhow::bail!(
                    "Parent directory of file {} doesn't exist in the file tree",
                    file_path.display()
                );
            }
        } else {
            anyhow::bail!("File's path component was not normal")
        }
    }

    fn add_subdirectory(
        &self,
        file_id: Uuid,
        hash: Option<u32>,
        file_path: impl AsRef<Path>,
        parent_node: Arc<Mutex<FileTreeNode>>,
    ) -> anyhow::Result<Arc<Mutex<FileTreeNode>>> {
        let file_path = file_path.as_ref();
        let mut parent_node = parent_node.lock().unwrap();

        // Check if this file is directly under the parent node's directory
        let file_parent = file_path.parent().unwrap();
        if file_parent == parent_node.path {
            if parent_node.children.contains_key(file_path) {
                anyhow::bail!("This path already exists in the file tree");
            }

            let file_name = file_path.file_name().unwrap().to_str().unwrap().to_owned();
            let new_node = FileTreeNode {
                id: file_id,
                name: file_name.clone(),
                path: file_path.to_owned(),
                parent: None,
                children: HashMap::new(),
                hash,
            };
            let value = Arc::new(Mutex::new(new_node));
            parent_node.children.insert(file_name.into(), value.clone());
            return Ok(value);
        }

        // File is under a subdirectory
        let components = file_parent
            .strip_prefix(&parent_node.path)?
            .components()
            .collect::<Vec<_>>();
        let first_comp = components.first().unwrap();
        if let Component::Normal(first_comp) = first_comp {
            let comp_path = Path::new(first_comp);

            if let Some(node) = parent_node.children.get_mut(comp_path).cloned() {
                self.add_subdirectory(file_id, hash, file_path, node)
            } else {
                anyhow::bail!(
                    "Parent directory of file {} doesn't exist in the file tree",
                    file_path.display()
                );
            }
        } else {
            anyhow::bail!("File's path component was not normal")
        }
    }
}
