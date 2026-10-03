use crate::errors::ApiError;
use crate::host::Host;
use crate::services::folders::resolve_folder;
use crate::services::pty::{CompletedRunSink, PtySubscriber};
use crate::services::runs::RunKind;
use std::sync::Arc;
use sworm_protocol::task::TaskDefinition;

impl Host {
    /// Return the parsed task list for a folder. Idempotently wires up
    /// the file watcher so callers receive task-change events when the
    /// folder's `.sworm/tasks.jsonc` is modified externally.
    pub fn tasks_list(&self, folder_path: String) -> Result<Vec<TaskDefinition>, ApiError> {
        let folder = resolve_folder(&folder_path)?;

        // Watcher setup is best-effort; a failure must not block task listing.
        if let Err(error) = self.tasks.watch(Arc::clone(&self.events), &folder) {
            tracing::warn!(
                "tasks watcher for {} failed to start: {}",
                folder.display(),
                error
            );
        }

        self.tasks.load(&folder).map_err(ApiError::Internal)
    }

    pub fn tasks_start(
        &self,
        run_id: String,
        folder_path: String,
        task_id: String,
        active_file_path: Option<String>,
        cols: u16,
        rows: u16,
        subscriber: Option<PtySubscriber>,
        owner_id: Option<String>,
        attach_only: bool,
    ) -> Result<(), ApiError> {
        let _owner = self.runs.owner_activity(owner_id.as_deref())?;
        let folder = resolve_folder(&folder_path)?;
        let kind = RunKind::Task {
            task_id: task_id.clone(),
        };
        self.runs.with_start(&run_id, |runs| {
            if runs.reuse(
                &run_id,
                &folder,
                &kind,
                &self.pty,
                subscriber.as_ref(),
                owner_id.as_deref(),
                cols,
                rows,
            )? {
                return Ok(());
            }
            if attach_only {
                return Err(ApiError::NotFound(format!("Run not found: {run_id}")));
            }
            let (_, completed) = runs.reserve(&run_id, folder.clone(), kind);
            let result = self.spawn_task(
                run_id.clone(),
                folder,
                task_id,
                active_file_path,
                cols,
                rows,
                subscriber,
                owner_id,
                completed,
            );
            if result.is_err() {
                runs.abort(&run_id);
            }
            result
        })
    }

    fn spawn_task(
        &self,
        run_id: String,
        folder: std::path::PathBuf,
        task_id: String,
        active_file_path: Option<String>,
        cols: u16,
        rows: u16,
        subscriber: Option<PtySubscriber>,
        owner_id: Option<String>,
        completed: Option<CompletedRunSink>,
    ) -> Result<(), ApiError> {
        let task = self
            .tasks
            .find(&folder, &task_id)
            .map_err(ApiError::Internal)?
            .ok_or_else(|| ApiError::NotFound(format!("Task not found: {task_id}")))?;

        let base_env = self.folder_env(&folder);
        let resolved = self
            .tasks
            .resolve(&task, &folder, active_file_path.as_deref(), &base_env);

        // Always shell-wrap so pipes, globs, `&&`, and quoted args work
        // exactly as a user would type them in their own terminal.
        let shell = self.env.detected_shell.clone();
        let shell_args = ["-c", resolved.command.as_str()];
        let cwd = resolved.cwd.to_string_lossy().into_owned();

        let on_exit = if task.singleton {
            let lease = self
                .tasks
                .register_singleton(folder.clone(), task_id.clone(), run_id.clone())
                .map_err(ApiError::Internal)?;
            let tasks = self.tasks.clone();
            let singleton_folder = folder.clone();
            let singleton_task_id = task_id.clone();
            Some(Box::new(move |_: &str, _: Option<i32>| {
                tasks.release_singleton(&singleton_folder, &singleton_task_id, &lease);
            }) as Box<dyn FnOnce(&str, Option<i32>) + Send>)
        } else {
            None
        };

        if let Err(error) = self.pty.spawn(
            run_id.clone(),
            &shell,
            &shell_args,
            Some(&cwd),
            Some(&resolved.env),
            cols,
            rows,
            subscriber,
            owner_id,
            on_exit,
            completed,
        ) {
            self.tasks.release_singleton_by_run_id(&run_id);
            return Err(ApiError::Pty(error));
        }
        Ok(())
    }

    pub fn tasks_stop(&self, run_id: String) -> Result<(), ApiError> {
        self.runs.with_run(&run_id, |runs| {
            runs.stop(
                &run_id,
                RunKind::Task {
                    task_id: String::new(),
                },
                &self.pty,
            )?;
            self.forget_run(&run_id);
            Ok(())
        })
    }
}
