pub mod lifecycle;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeServerInfo {
    pub pid: u32,
    pub port: u16,
    pub workdir: String,
}
