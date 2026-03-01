use std::io::{SeekFrom, Read, Seek};
use anyhow::{Result, anyhow};
use module::Module;

use byteorder::ReadBytesExt;
use crate::{format_it::ITModule, format_s3m::S3MModule, format_stm::STMModule, module::ModuleInterface};

pub mod format_it;
pub mod format_s3m;
pub mod format_stm;
pub mod module;
pub mod player;
pub mod stm_tools;

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

    // Try STM
    {
        let mut magic_stm = [0u8;8];
        let mut valid_characters = 0;
        reader.seek(SeekFrom::Start(0x14))?;
        reader.read_exact(&mut magic_stm)?;
        let end_of_id = reader.read_u8()?;
        let song_type = reader.read_u8()?;
        let major_version = reader.read_u8()?;
        for letter in magic_stm {
            if letter >= 0x20 && letter <= 0x7F {
                valid_characters += 1;
            }
        }
        reader.seek(SeekFrom::Start(0))?;
        if valid_characters == 8 && (end_of_id == 0x1A || end_of_id == 0x02) && song_type == 2 && major_version == 2 {
            let stm = STMModule::load(reader)?;
            return Ok(stm.module());
        }

    }

    Err(anyhow!("Unrecognized or unsupported module format"))
}