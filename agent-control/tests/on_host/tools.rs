pub mod config;
pub mod custom_agent_type;
pub mod instance_id;

use std::error::Error;
use std::fs::create_dir_all;
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};
pub fn create_temp_file(
    dir: &Path,
    file_name: &str,
    data: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    create_dir_all(dir)?;
    let file_path = dir.join(file_name);
    let mut file = File::create(&file_path)?;
    writeln!(file, "{data}")?;
    Ok(file_path)
}
