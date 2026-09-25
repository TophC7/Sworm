use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRuntimeInfo {
    pub name: String,
    pub version: String,
    pub memory_bytes: Option<u64>,
    pub app_cpu_time_ticks: Option<u64>,
    pub system_cpu_time_ticks: Option<u64>,
    pub thread_count: Option<u32>,
    pub file_descriptor_count: Option<u32>,
}
