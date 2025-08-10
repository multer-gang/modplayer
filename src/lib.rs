use std::io::{SeekFrom, Read, Seek};
use anyhow::{Result, anyhow};
use module::Module;

use crate::{format_it::ITModule, format_s3m::S3MModule, module::ModuleInterface};

pub mod format_it;
pub mod format_s3m;
pub mod module;
pub mod player;

// Load a module by probing its header
pub fn load_module(mut reader: impl Read + Seek) -> Result<Module> {
    // Try IT first by magic at start
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic)?;
    reader.seek(SeekFrom::Start(0))?;
    if &magic == b"IMPM" {
        let it = ITModule::load(reader)?;
        return Ok(it.module());
    }

    // Try S3M by magic at 0x2C
    reader.seek(SeekFrom::Start(0x2C))?;
    reader.read_exact(&mut magic)?;
    reader.seek(SeekFrom::Start(0))?;
    if &magic == b"SCRM" {
        let s3m = S3MModule::load(reader)?;
        return Ok(s3m.module());
    }

    Err(anyhow!("Unrecognized or unsupported module format"))
}