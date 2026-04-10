use crate::models::project::Project;
use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

/// Project service: CRUD for persisted projects.
pub struct ProjectService;

impl ProjectService {
    pub fn new() -> Self {
        Self
    }

    /// Add a new project from a validated git repo path.
    pub fn add(
        &self,
        conn: &Connection,
        name: &str,
        path: &str,
        default_branch: Option<&str>,
        base_ref: Option<&str>,
    ) -> Result<Project, String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO projects (id, name, path, default_branch, base_ref, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![id, name, path, default_branch, base_ref, now, now],
        )
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                format!("A project with path '{}' already exists", path)
            } else {
                format!("Failed to add project: {}", e)
            }
        })?;

        Ok(Project {
            id,
            name: name.to_string(),
            path: path.to_string(),
            default_branch: default_branch.map(|s| s.to_string()),
            base_ref: base_ref.map(|s| s.to_string()),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    /// List all projects, most recently updated first.
    pub fn list(&self, conn: &Connection) -> Result<Vec<Project>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT id, name, path, default_branch, base_ref, created_at, updated_at
                 FROM projects ORDER BY updated_at DESC",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    default_branch: row.get(3)?,
                    base_ref: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })
            .map_err(|e| format!("Failed to query projects: {}", e))?;

        let mut projects = Vec::new();
        for row in rows {
            match row {
                Ok(project) => projects.push(project),
                Err(error) => {
                    tracing::warn!("Dropping unreadable project row: {}", error);
                }
            }
        }

        Ok(projects)
    }

    /// Get a single project by ID.
    pub fn get(&self, conn: &Connection, id: &str) -> Result<Option<Project>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT id, name, path, default_branch, base_ref, created_at, updated_at
                 FROM projects WHERE id = ?1",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let result = stmt
            .query_row(rusqlite::params![id], |row| {
                Ok(Project {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    default_branch: row.get(3)?,
                    base_ref: row.get(4)?,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })
            .optional()
            .map_err(|e| format!("Failed to get project: {}", e))?;

        Ok(result)
    }

    /// Remove a project by ID.
    pub fn remove(&self, conn: &Connection, id: &str) -> Result<(), String> {
        conn.execute("DELETE FROM projects WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| format!("Failed to remove project: {}", e))?;
        Ok(())
    }
}

use rusqlite::OptionalExtension;
