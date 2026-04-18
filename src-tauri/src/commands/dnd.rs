use crate::errors::ApiError;
use uuid::Uuid;

#[tauri::command]
pub fn dnd_save_dropped_bytes(bytes: Vec<u8>, suggested_name: String) -> Result<String, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::InvalidArgument(
            "Dropped image payload is empty".to_string(),
        ));
    }

    let temp_dir = std::env::temp_dir().join("sworm-drops");
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| ApiError::Io(format!("Failed to create temp drop directory: {}", e)))?;

    let safe_name = sanitize_filename(&suggested_name);
    let file_path = temp_dir.join(format!("{}-{}", Uuid::new_v4(), safe_name));
    std::fs::write(&file_path, bytes)
        .map_err(|e| ApiError::Io(format!("Failed to write dropped bytes: {}", e)))?;

    Ok(file_path.to_string_lossy().into_owned())
}

fn sanitize_filename(input: &str) -> String {
    let candidate = input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();

    if candidate.trim_matches('_').is_empty() {
        "dropped-image.png".to_string()
    } else {
        candidate
    }
}
