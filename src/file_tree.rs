use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::Mutex,
    sync::{Arc, Weak},
};

use uuid::Uuid;

pub struct FileTreeNode {
    pub id: Uuid,
    pub name: String,
    pub path: PathBuf,
    pub parent: Option<Weak<FileTreeNode>>,
    pub children: HashMap<PathBuf, Arc<Mutex<FileTreeNode>>>,
}

pub struct FileTree {
    pub project_id: Uuid,
    pub root_dir: PathBuf,
    pub children: HashMap<PathBuf, Arc<Mutex<FileTreeNode>>>,
}

impl FileTree {
    fn add_inner(
        &self,
        file_id: Uuid,
        file_path: impl AsRef<Path>,
        parent_node: Arc<Mutex<FileTreeNode>>,
    ) -> anyhow::Result<()> {
        let file_path = file_path.as_ref();
        let mut parent_node = parent_node.lock().unwrap();

        // Check if this file is directly under the parent node's directory
        let file_parent = file_path.parent().unwrap();
        if file_parent == parent_node.path {
            if parent_node.children.contains_key(file_path) {
                anyhow::bail!("This path already exists in the file tree");
            }

            let new_node = FileTreeNode {
                id: file_id,
                name: file_path.file_name().unwrap().to_str().unwrap().to_owned(),
                path: file_path.to_owned(),
                parent: None,
                children: HashMap::new(),
            };
            parent_node
                .children
                .insert(file_path.to_owned(), Arc::new(Mutex::new(new_node)));
            return Ok(());
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
                self.add_inner(file_id, file_path, node)?;
            } else {
                anyhow::bail!(
                    "Parent directory of file {} doesn't exist in the file tree",
                    file_path.display()
                );
            }
        } else {
            anyhow::bail!("File's path component was not normal")
        }

        Ok(())
    }

    pub fn add(&mut self, file_id: Uuid, file_path: impl AsRef<Path>) -> anyhow::Result<()> {
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
            };
            self.children
                .insert(file_path.to_owned(), Arc::new(Mutex::new(new_node)));
            return Ok(());
        }

        // File is under a subdirectory
        let components = file_parent.components().collect::<Vec<_>>();
        let first_comp = components.first().unwrap();
        if let Component::Normal(first_comp) = first_comp {
            let comp_path = Path::new(first_comp);

            if let Some(node) = self.children.get_mut(comp_path).cloned() {
                self.add_inner(file_id, file_path, node)?;
            } else {
                anyhow::bail!(
                    "Parent directory of file {} doesn't exist in the file tree",
                    file_path.display()
                );
            }
        } else {
            anyhow::bail!("File's path component was not normal")
        }

        Ok(())
    }
}
